//! History recording, spike detection and alert evaluation. Runs on the monitor thread, once per tick.

use crate::monitor::{ProcRow, SystemSnap};
use crate::settings::Settings;
use crate::store::{SampleRow, SnapRow, Store};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum Event {
    Alert { key: String, kind: String, title: String, body: String },
    Spike { kind: String, summary: String },
}

#[derive(Default)]
struct Acc {
    n: u32,
    cpu: f64,
    gpu_n: u32,
    gpu_util: f64,
    gpu_temp: Option<f64>,
    gpu_power: f64,
    gpu_power_n: u32,
    disk_r: f64,
    disk_w: f64,
    disk_n: u32,
    net_rx: f64,
    net_tx: f64,
    net_n: u32,
    ram_avail_min: Option<f64>,
}

pub struct Recorder {
    store: Arc<Store>,
    acc: Acc,
    last_sample: Instant,
    last_snap: Instant,
    last_flush: Instant,
    last_prune: Instant,
    ring: VecDeque<(Instant, f64)>, // ram used bytes
    vram_ring: VecDeque<(Instant, f64)>,
    proc_hist: VecDeque<(Instant, i64, HashMap<(u32, u64), (String, u64)>)>,
    last_event: HashMap<String, Instant>,
    cpu_over: Option<Instant>,
    cpu_alert_over: Option<Instant>,
    gpu_over: Option<Instant>,
    mem_over: Option<Instant>,
    gpu_temp_over: Option<Instant>,
    last_disk_check: Instant,
    last_tick: Instant,
}

pub fn fmt_b(b: f64) -> String {
    let b = b.max(0.0);
    if b >= 1073741824.0 {
        format!("{:.1} GB", b / 1073741824.0)
    } else if b >= 1048576.0 {
        format!("{:.0} MB", b / 1048576.0)
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

impl Recorder {
    pub fn new(store: Arc<Store>) -> Recorder {
        let past = Instant::now() - Duration::from_secs(3600);
        let mut last_event: HashMap<String, Instant> = HashMap::new();
        let now_ms = crate::monitor::now_ms() as i64;
        for (key, ts) in store.recent_alert_keys(now_ms - 24 * 3_600_000) {
            if let Some(t) = Instant::now().checked_sub(Duration::from_millis((now_ms - ts).max(0) as u64)) {
                last_event.insert(key, t);
            }
        }
        Recorder {
            store,
            acc: Acc::default(),
            last_sample: Instant::now(),
            last_snap: past,
            last_flush: Instant::now(),
            last_prune: past,
            ring: VecDeque::new(),
            vram_ring: VecDeque::new(),
            proc_hist: VecDeque::new(),
            last_event,
            cpu_over: None,
            cpu_alert_over: None,
            gpu_over: None,
            mem_over: None,
            gpu_temp_over: None,
            last_disk_check: past,
            last_tick: Instant::now(),
        }
    }

    fn cooled(&mut self, key: &str, secs: u64) -> bool {
        match self.last_event.get(key) {
            Some(t) if t.elapsed().as_secs() < secs => false,
            _ => {
                self.last_event.insert(key.to_string(), Instant::now());
                true
            }
        }
    }

    fn select_snap_rows(procs: &[ProcRow]) -> Vec<SnapRow> {
        let mut idx: Vec<usize> = (0..procs.len()).filter(|&i| procs[i].private_ws > 0 && procs[i].pid != 0).collect();
        let mut chosen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        idx.sort_by(|&a, &b| procs[b].private_ws.cmp(&procs[a].private_ws));
        chosen.extend(idx.iter().take(20));
        idx.sort_by(|&a, &b| procs[b].cpu.partial_cmp(&procs[a].cpu).unwrap_or(std::cmp::Ordering::Equal));
        chosen.extend(idx.iter().take(5).filter(|&&i| procs[i].cpu >= 1.0));
        idx.sort_by(|&a, &b| procs[b].gpu_mem.unwrap_or(0).cmp(&procs[a].gpu_mem.unwrap_or(0)));
        chosen.extend(idx.iter().take(5).filter(|&&i| procs[i].gpu_mem.unwrap_or(0) > 64 << 20));
        chosen
            .into_iter()
            .map(|i| {
                let p = &procs[i];
                SnapRow { pid: p.pid, ct: p.create_time, name: p.name.clone(), pws: p.private_ws, ws: p.working_set, commit: p.commit, cpu: p.cpu, gpu: p.gpu, gpu_mem: p.gpu_mem }
            })
            .collect()
    }

    /// Attribute a RAM rise to applications by comparing the process table now with the snapshot closest to the
    /// moment usage was at its lowest (the spike's baseline), so both sides describe the same interval.
    fn detail(&self, snap: &SystemSnap, procs: &[ProcRow], baseline: Instant, rise: f64) -> (serde_json::Value, String) {
        let used_now = (snap.mem.total - snap.mem.available) as f64;
        let mut by_app: HashMap<String, (String, u32, u64, f64, u64)> = HashMap::new();
        for p in procs {
            if p.pid == 0 {
                continue;
            }
            let e = by_app.entry(p.name.to_ascii_lowercase()).or_insert((p.name.clone(), 0, 0, 0.0, 0));
            e.1 += 1;
            e.2 += p.private_ws;
            e.3 += p.cpu;
            e.4 += p.gpu_mem.unwrap_or(0);
        }
        let mut top: Vec<_> = by_app.values().cloned().collect();
        top.sort_by(|a, b| b.2.cmp(&a.2));
        let top_now: Vec<_> = top.iter().take(12).map(|t| json!({"name": t.0, "count": t.1, "privateWs": t.2, "cpu": t.3, "gpuMem": t.4})).collect();

        // nearest recorded process snapshot to the baseline moment (within 45 s)
        let best = self
            .proc_hist
            .iter()
            .map(|(t, ts, m)| (if *t > baseline { t.duration_since(baseline) } else { baseline.duration_since(*t) }, t, ts, m))
            .min_by_key(|x| x.0)
            .filter(|x| x.0 <= Duration::from_secs(45));
        let mut growth: Vec<(String, u32, i64, u64, u32)> = Vec::new();
        let mut window_secs = 0u64;
        let mut net_delta = 0i64;
        let mut attributed = false;
        if let Some((_, t_old, ts_old, old)) = best {
            attributed = true;
            window_secs = Instant::now().duration_since(*t_old).as_secs();
            let old_ft = (*ts_old + 11_644_473_600_000) as u64 * 10_000; // unix ms -> FILETIME
            // app -> (display name, running count, signed delta, now total, processes new since baseline)
            let mut now_app: HashMap<String, (String, u32, i64, u64, u32)> = HashMap::new();
            for p in procs {
                if p.pid == 0 {
                    continue;
                }
                let before = old.get(&(p.pid, p.create_time)).map(|x| x.1 as i64);
                let is_new = before.is_none() && p.create_time > old_ft;
                let e = now_app.entry(p.name.to_ascii_lowercase()).or_insert((p.name.clone(), 0, 0, 0, 0));
                e.1 += 1;
                e.2 += p.private_ws as i64 - before.unwrap_or(0);
                e.3 += p.private_ws;
                if is_new {
                    e.4 += 1;
                }
            }
            // processes that exited since the baseline give their memory back
            let alive: std::collections::HashSet<(u32, u64)> = procs.iter().map(|p| (p.pid, p.create_time)).collect();
            for ((pid, ct), (name, ws)) in old.iter() {
                if !alive.contains(&(*pid, *ct)) {
                    let e = now_app.entry(name.to_ascii_lowercase()).or_insert((name.clone(), 0, 0, 0, 0));
                    e.2 -= *ws as i64;
                }
            }
            net_delta = now_app.values().map(|v| v.2).sum();
            growth = now_app.into_values().filter(|v| v.2 > 50 << 20).collect();
            growth.sort_by(|a, b| b.2.cmp(&a.2));
        }
        let unexplained = rise as i64 - net_delta;
        let top_growth: Vec<_> = growth.iter().take(8).map(|g| json!({"name": g.0, "count": g.1, "delta": g.2, "now": g.3, "newProcesses": g.4})).collect();
        let mut text = String::new();
        if attributed {
            if let Some(g) = growth.first() {
                text = format!(" Largest growth: {} +{}", g.0, fmt_b(g.2 as f64));
                if let Some(g2) = growth.get(1) {
                    text.push_str(&format!(", {} +{}", g2.0, fmt_b(g2.2 as f64)));
                }
                text.push('.');
            } else {
                text = " No single application grew by 50 MB or more.".to_string();
            }
            if rise > 0.0 && unexplained as f64 > rise * 0.3 {
                text.push_str(&format!(" About {} of the rise is not visible as process private memory (kernel, cache, shared or small processes).", fmt_b(unexplained as f64)));
            }
        } else if let Some(t) = top.first() {
            text = format!(" No process snapshot near the start of the rise; largest process group right now: {} ({}).", t.0, fmt_b(t.2 as f64));
        }
        (json!({"topNow": top_now, "topGrowth": top_growth, "ramUsed": used_now, "ramTotal": snap.mem.total, "windowSecs": window_secs, "riseBytes": rise, "attributedToProcesses": net_delta, "unexplainedBytes": unexplained, "hasBaseline": attributed}), text)
    }

    pub fn on_tick(&mut self, snap: &SystemSnap, procs: &[ProcRow], s: &Settings) -> Vec<Event> {
        let mut events = Vec::new();
        let now = Instant::now();
        let ts = snap.ts as i64;
        // after a pause / sleep the sustain timers and baselines describe time we did not observe
        if now.duration_since(self.last_tick) > Duration::from_secs(10).max(Duration::from_millis(s.poll_interval_ms * 4)) {
            self.cpu_over = None;
            self.cpu_alert_over = None;
            self.gpu_over = None;
            self.mem_over = None;
            self.gpu_temp_over = None;
            self.ring.clear();
            self.vram_ring.clear();
            self.acc = Acc::default();
        }
        self.last_tick = now;
        let used = snap.mem.in_use as f64 + snap.mem.modified.unwrap_or(0) as f64; // total - available
        let gpu_util = snap.gpu.as_ref().and_then(|g| g.util.value);
        let vram = snap.gpu.as_ref().and_then(|g| g.vram_used.value);
        let gpu_temp = snap.gpu.as_ref().and_then(|g| g.temp.value);

        // ---- accumulate for the sample row ----
        self.acc.n += 1;
        self.acc.cpu += snap.cpu.total;
        if let Some(u) = gpu_util {
            self.acc.gpu_n += 1;
            self.acc.gpu_util += u;
        }
        if let Some(t) = gpu_temp {
            self.acc.gpu_temp = Some(self.acc.gpu_temp.map_or(t, |x| x.max(t)));
        }
        if let Some(p) = snap.gpu.as_ref().and_then(|g| g.power_w.value) {
            self.acc.gpu_power += p;
            self.acc.gpu_power_n += 1;
        }
        if let (Some(r), Some(w)) = (snap.disk_io.read_bps, snap.disk_io.write_bps) {
            self.acc.disk_r += r;
            self.acc.disk_w += w;
            self.acc.disk_n += 1;
        }
        if let (Some(r), Some(t)) = (snap.net.rx_bps, snap.net.tx_bps) {
            self.acc.net_rx += r;
            self.acc.net_tx += t;
            self.acc.net_n += 1;
        }
        let av = snap.mem.available as f64;
        self.acc.ram_avail_min = Some(self.acc.ram_avail_min.map_or(av, |x| x.min(av)));

        // ---- rings for spike detection ----
        self.ring.push_back((now, used));
        while self.ring.front().map_or(false, |(t, _)| now.duration_since(*t) > Duration::from_secs(180)) {
            self.ring.pop_front();
        }
        if let Some(v) = vram {
            self.vram_ring.push_back((now, v));
            while self.vram_ring.front().map_or(false, |(t, _)| now.duration_since(*t) > Duration::from_secs(120)) {
                self.vram_ring.pop_front();
            }
        }

        // ---- process snapshot ----
        let snap_due = now.duration_since(self.last_snap).as_secs() >= s.proc_snapshot_interval_s;
        if snap_due {
            self.last_snap = now;
            if s.record_enabled {
                self.store.push_snap(ts, Self::select_snap_rows(procs));
            }
            let m: HashMap<(u32, u64), (String, u64)> = procs.iter().filter(|p| p.private_ws > 4 << 20).map(|p| ((p.pid, p.create_time), (p.name.clone(), p.private_ws))).collect();
            self.proc_hist.push_back((now, ts, m));
            while self.proc_hist.front().map_or(false, |(t, _, _)| now.duration_since(*t) > Duration::from_secs(300)) {
                self.proc_hist.pop_front();
            }
        }

        // ---- sample row ----
        if now.duration_since(self.last_sample).as_secs() >= s.record_interval_s && self.acc.n > 0 {
            self.last_sample = now;
            let a = std::mem::take(&mut self.acc);
            if s.record_enabled {
                let row = SampleRow {
                    ts,
                    cpu: Some(a.cpu / a.n as f64),
                    ram_used: Some(used),
                    ram_avail: a.ram_avail_min,
                    commit: Some(snap.mem.commit_total as f64),
                    gpu_util: if a.gpu_n > 0 { Some(a.gpu_util / a.gpu_n as f64) } else { None },
                    vram_used: vram,
                    gpu_temp: a.gpu_temp,
                    gpu_power: if a.gpu_power_n > 0 { Some(a.gpu_power / a.gpu_power_n as f64) } else { None },
                    cpu_temp: snap.cpu_temp.value,
                    disk_r: if a.disk_n > 0 { Some(a.disk_r / a.disk_n as f64) } else { None },
                    disk_w: if a.disk_n > 0 { Some(a.disk_w / a.disk_n as f64) } else { None },
                    net_rx: if a.net_n > 0 { Some(a.net_rx / a.net_n as f64) } else { None },
                    net_tx: if a.net_n > 0 { Some(a.net_tx / a.net_n as f64) } else { None },
                };
                self.store.push_sample(row);
            }
        }
        if now.duration_since(self.last_flush).as_secs() >= 30 {
            self.last_flush = now;
            self.store.flush();
        }
        if now.duration_since(self.last_prune).as_secs() >= 3600 {
            self.last_prune = now;
            self.store.prune(s.retention_days);
        }

        // ---- memory spike: rise over the trailing 2 minutes ----
        let total = snap.mem.total as f64;
        let (t_min, min_recent) = self.ring.iter().filter(|(t, _)| now.duration_since(*t) <= Duration::from_secs(120)).fold((now, f64::MAX), |acc, x| if x.1 < acc.1 { (x.0, x.1) } else { acc });
        let thresh = (s.spike_mem_gb * 1073741824.0).max(total * 0.04);
        if min_recent < f64::MAX && used - min_recent >= thresh && self.cooled("spike-mem", 120) {
            let (detail, text) = self.detail(snap, procs, t_min, used - min_recent);
            let summary = format!("RAM use rose {} within 2 minutes (from {} to {}, {:.0}% of RAM).{}", fmt_b(used - min_recent), fmt_b(min_recent), fmt_b(used), used * 100.0 / total, text);
            if s.record_enabled {
                self.store.push_snap(ts, Self::select_snap_rows(procs));
                self.store.add_spike(ts, "memory", used, Some(min_recent), &summary, &detail);
            }
            events.push(Event::Spike { kind: "memory".into(), summary });
        }

        // ---- CPU spike: >=90% for 15 s ----
        if snap.cpu.total >= 90.0 {
            let since = *self.cpu_over.get_or_insert(now);
            if now.duration_since(since).as_secs() >= 15 && self.cooled("spike-cpu", 300) {
                let mut top: Vec<&ProcRow> = procs.iter().filter(|p| p.pid != 0).collect();
                top.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
                let list: Vec<_> = top.iter().take(8).map(|p| json!({"name": p.name, "pid": p.pid, "cpu": p.cpu, "privateWs": p.private_ws})).collect();
                let summary = format!("CPU stayed at {:.0}% or above for 15 seconds. Busiest process: {} ({:.0}% of total CPU).", 90.0, top.first().map(|p| p.name.as_str()).unwrap_or("?"), top.first().map(|p| p.cpu).unwrap_or(0.0));
                if s.record_enabled {
                    self.store.push_snap(ts, Self::select_snap_rows(procs));
                    self.store.add_spike(ts, "cpu", snap.cpu.total, None, &summary, &json!({"topCpu": list}));
                }
                events.push(Event::Spike { kind: "cpu".into(), summary });
            }
        } else {
            self.cpu_over = None;
        }

        // ---- GPU / VRAM ----
        if let Some(u) = gpu_util {
            if u >= 95.0 {
                let since = *self.gpu_over.get_or_insert(now);
                if now.duration_since(since).as_secs() >= 15 && self.cooled("spike-gpu", 600) {
                    let mut top: Vec<&ProcRow> = procs.iter().filter(|p| p.gpu.unwrap_or(0.0) > 0.5).collect();
                    top.sort_by(|a, b| b.gpu.partial_cmp(&a.gpu).unwrap_or(std::cmp::Ordering::Equal));
                    let list: Vec<_> = top.iter().take(6).map(|p| json!({"name": p.name, "pid": p.pid, "gpu": p.gpu, "gpuMem": p.gpu_mem})).collect();
                    let summary = format!("GPU utilization stayed at 95% or above for 15 seconds.{}", top.first().map(|p| format!(" Highest GPU process: {}.", p.name)).unwrap_or_default());
                    if s.record_enabled {
                        self.store.push_snap(ts, Self::select_snap_rows(procs));
                        self.store.add_spike(ts, "gpu", u, None, &summary, &json!({"topGpu": list}));
                    }
                    events.push(Event::Spike { kind: "gpu".into(), summary });
                }
            } else {
                self.gpu_over = None;
            }
        }
        if let Some(v) = vram {
            let min = self.vram_ring.iter().map(|x| x.1).fold(f64::MAX, f64::min);
            if min < f64::MAX && v - min >= 2.0 * 1073741824.0 && self.cooled("spike-vram", 300) {
                let mut top: Vec<&ProcRow> = procs.iter().filter(|p| p.gpu_mem.unwrap_or(0) > 0).collect();
                top.sort_by(|a, b| b.gpu_mem.cmp(&a.gpu_mem));
                let list: Vec<_> = top.iter().take(6).map(|p| json!({"name": p.name, "pid": p.pid, "gpuMem": p.gpu_mem})).collect();
                let summary = format!("VRAM use rose {} within 2 minutes (to {}).{}", fmt_b(v - min), fmt_b(v), top.first().map(|p| format!(" Largest VRAM user: {} ({}).", p.name, fmt_b(p.gpu_mem.unwrap_or(0) as f64))).unwrap_or_default());
                if s.record_enabled {
                    self.store.add_spike(ts, "vram", v, Some(min), &summary, &json!({"topVram": list}));
                }
                events.push(Event::Spike { kind: "vram".into(), summary });
            }
        }

        // ---- alerts ----
        let cd = s.alert_cooldown_min * 60;
        if s.alert_mem_enabled {
            let pct = used * 100.0 / total.max(1.0);
            if pct >= s.alert_mem_pct {
                let since = *self.mem_over.get_or_insert(now);
                if now.duration_since(since).as_secs() >= s.alert_mem_sustain_s && self.cooled("alert-mem", cd) {
                    let mut app: HashMap<String, (String, u64)> = HashMap::new();
                    for p in procs {
                        let e = app.entry(p.name.to_ascii_lowercase()).or_insert((p.name.clone(), 0));
                        e.1 += p.private_ws;
                    }
                    let mut v: Vec<_> = app.into_values().collect();
                    v.sort_by(|a, b| b.1.cmp(&a.1));
                    let tops: Vec<String> = v.iter().take(3).map(|x| format!("{} {}", x.0, fmt_b(x.1 as f64))).collect();
                    events.push(Event::Alert { key: "alert-mem".into(), kind: "memory".into(), title: format!("Memory pressure: RAM at {:.0}%", pct), body: format!("{} of {} in use for over {}s. Largest: {}.", fmt_b(used), fmt_b(total), s.alert_mem_sustain_s, tops.join(", ")) });
                }
            } else {
                self.mem_over = None;
            }
        }
        if s.alert_cpu_enabled {
            if snap.cpu.total >= s.alert_cpu_pct {
                let since = *self.cpu_alert_over.get_or_insert(now);
                if now.duration_since(since).as_secs() >= s.alert_cpu_sustain_s && self.cooled("alert-cpu", cd) {
                    events.push(Event::Alert { key: "alert-cpu".into(), kind: "cpu".into(), title: format!("CPU at {:.0}%", snap.cpu.total), body: format!("CPU stayed above {:.0}% for {} seconds.", s.alert_cpu_pct, s.alert_cpu_sustain_s) });
                }
            } else {
                self.cpu_alert_over = None;
            }
        }
        if s.alert_gpu_temp_enabled {
            if let Some(t) = gpu_temp {
                if t >= s.alert_gpu_temp_c {
                    let since = *self.gpu_temp_over.get_or_insert(now);
                    if now.duration_since(since).as_secs() >= 10 && self.cooled("alert-gpu-temp", cd) {
                        events.push(Event::Alert { key: "alert-gpu-temp".into(), kind: "gpu-temp".into(), title: format!("GPU temperature {:.0}°C", t), body: format!("GPU temperature reached {:.0}°C (alert threshold {:.0}°C).", t, s.alert_gpu_temp_c) });
                    }
                } else {
                    self.gpu_temp_over = None;
                }
            }
        }
        if s.alert_disk_enabled && now.duration_since(self.last_disk_check).as_secs() >= 60 {
            self.last_disk_check = now;
            for d in &snap.disks {
                if d.kind != "fixed" || d.total == 0 {
                    continue;
                }
                let free_pct = d.free as f64 * 100.0 / d.total as f64;
                if free_pct < s.alert_disk_free_pct && self.cooled(&format!("alert-disk-{}", d.letter), cd.max(6 * 3600)) {
                    events.push(Event::Alert { key: format!("alert-disk-{}", d.letter), kind: "disk".into(), title: format!("{} is almost full", d.letter), body: format!("{} has {} free of {} ({:.1}%).", d.letter, fmt_b(d.free as f64), fmt_b(d.total as f64), free_pct) });
                }
            }
        }
        for e in &events {
            if let Event::Alert { key, title, body, .. } = e {
                self.store.add_alert(ts, key, &format!("{} — {}", title, body));
            }
        }
        events
    }
}
