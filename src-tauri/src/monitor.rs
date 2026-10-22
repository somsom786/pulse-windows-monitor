//! Telemetry collector: one sampling thread produces a `SystemSnap` and a process table per tick.

use crate::gpu::{Gpu, GpuSnap, Reading};
use crate::pdh::Pdh;
use crate::winsys::{self, Drive, RawProc};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuSnap {
    pub name: String,
    pub logical: usize,
    pub physical: usize,
    pub total: f64,
    pub per_core: Vec<f64>,
    pub freq_mhz: Option<f64>,
    pub base_mhz: Option<f64>,
    pub per_core_freq: Vec<Option<f64>>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageFileSnap {
    pub name: String,
    pub total: u64,
    pub in_use: u64,
    pub peak: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemSnap {
    pub total: u64,
    pub available: u64,
    /// total - available - modified: memory actively held by processes, drivers and the kernel
    pub in_use: u64,
    pub modified: Option<u64>,
    pub standby: Option<u64>,
    pub free: Option<u64>,
    pub compressed: Option<u64>,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub commit_peak: u64,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
    pub system_cache: u64,
    pub pagefiles: Vec<PageFileSnap>,
    pub pages_in_per_sec: Option<f64>,
    pub pages_out_per_sec: Option<f64>,
    pub handles: u32,
    pub processes: u32,
    pub threads: u32,
    pub pressure: String,
    pub pressure_reason: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiskSnap {
    pub letter: String,
    pub label: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub free: u64,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub active_pct: Option<f64>,
    pub latency_ms: Option<f64>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdapterRate {
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub link_bps: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetSnap {
    pub rx_bps: Option<f64>,
    pub tx_bps: Option<f64>,
    pub adapters: Vec<AdapterRate>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiskIoTotal {
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub active_pct: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnap {
    pub ts: u64,
    pub uptime_s: u64,
    pub cpu: CpuSnap,
    pub cpu_temp: Reading,
    pub cpu_power: Reading,
    pub mem: MemSnap,
    pub gpu: Option<GpuSnap>,
    pub gpu_status: String,
    pub disks: Vec<DiskSnap>,
    pub disk_io: DiskIoTotal,
    pub net: NetSnap,
    pub poll_ms: u64,
    pub sample_cost_ms: f64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcRow {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub path: String,
    #[serde(serialize_with = "ser_u64_string")]
    pub create_time: u64,
    pub working_set: u64,
    pub private_ws: u64,
    pub commit: u64,
    pub peak_ws: u64,
    pub cpu: f64,
    pub io_read_bps: f64,
    pub io_write_bps: f64,
    pub gpu: Option<f64>,
    pub gpu_mem: Option<u64>,
    pub threads: u32,
    pub handles: u32,
    pub session: u32,
}

struct PdhIdx {
    mem_modified: Option<usize>,
    mem_standby: [Option<usize>; 3],
    mem_free: Option<usize>,
    pages_in: Option<usize>,
    pages_out: Option<usize>,
    cpu_perf_total: Option<usize>,
    cpu_freq_total: Option<usize>,
    cpu_perf_each: Option<usize>,
    ld_read: Option<usize>,
    ld_write: Option<usize>,
    ld_active: Option<usize>,
    ld_lat: Option<usize>,
    pd_read: Option<usize>,
    pd_write: Option<usize>,
    pd_active: Option<usize>,
    net_rx: Option<usize>,
    net_tx: Option<usize>,
    net_bw: Option<usize>,
    gpu_eng: Option<usize>,
    gpu_mem: Option<usize>,
    thermal: Option<usize>,
}

pub struct Collector {
    pdh: Option<Pdh>,
    idx: PdhIdx,
    gpu: Option<Gpu>,
    gpu_err: String,
    nproc_buf: Vec<u8>,
    prev_procs: HashMap<(u32, u64), (u64, u64, u64)>, // (cpu_time, read, write)
    prev_ts: Option<std::time::Instant>,
    prev_cpu: Vec<winsys::CpuTimes>,
    path_cache: HashMap<(u32, u64), String>,
    cpu_name: String,
    logical: usize,
    physical: usize,
    drives: Vec<Drive>,
    drives_at: std::time::Instant,
    pub thermal_zones: Vec<(String, f64)>,
    pub have_pdh_gpu: bool,
}

fn physical_cores() -> usize {
    use windows::Win32::System::SystemInformation::*;
    unsafe {
        let mut len = 0u32;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return 0;
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(RelationProcessorCore, Some(buf.as_mut_ptr() as *mut _), &mut len).is_err() {
            return 0;
        }
        let mut n = 0;
        let mut off = 0usize;
        while off < len as usize {
            let p = buf.as_ptr().add(off) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
            n += 1;
            let sz = (*p).Size as usize;
            if sz == 0 {
                break;
            }
            off += sz;
        }
        n
    }
}

fn cpu_brand() -> String {
    use winreg::enums::*;
    use winreg::RegKey;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0")
        .and_then(|k| k.get_value::<String, _>("ProcessorNameString"))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|_| "Unknown CPU".into())
}

impl Collector {
    pub fn new() -> Collector {
        let mut pdh = Pdh::new();
        let idx = if let Some(p) = pdh.as_mut() {
            PdhIdx {
                mem_modified: p.add("\\Memory\\Modified Page List Bytes"),
                mem_standby: [
                    p.add("\\Memory\\Standby Cache Reserve Bytes"),
                    p.add("\\Memory\\Standby Cache Normal Priority Bytes"),
                    p.add("\\Memory\\Standby Cache Core Bytes"),
                ],
                mem_free: p.add("\\Memory\\Free & Zero Page List Bytes"),
                pages_in: p.add("\\Memory\\Pages Input/sec"),
                pages_out: p.add("\\Memory\\Pages Output/sec"),
                cpu_perf_total: p.add("\\Processor Information(_Total)\\% Processor Performance"),
                cpu_freq_total: p.add("\\Processor Information(_Total)\\Processor Frequency"),
                cpu_perf_each: p.add("\\Processor Information(*)\\% Processor Performance"),
                ld_read: p.add("\\LogicalDisk(*)\\Disk Read Bytes/sec"),
                ld_write: p.add("\\LogicalDisk(*)\\Disk Write Bytes/sec"),
                ld_active: p.add("\\LogicalDisk(*)\\% Disk Time"),
                ld_lat: p.add("\\LogicalDisk(*)\\Avg. Disk sec/Transfer"),
                pd_read: p.add("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"),
                pd_write: p.add("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"),
                pd_active: p.add("\\PhysicalDisk(_Total)\\% Disk Time"),
                net_rx: p.add("\\Network Interface(*)\\Bytes Received/sec"),
                net_tx: p.add("\\Network Interface(*)\\Bytes Sent/sec"),
                net_bw: p.add("\\Network Interface(*)\\Current Bandwidth"),
                gpu_eng: p.add("\\GPU Engine(*)\\Utilization Percentage"),
                gpu_mem: p.add("\\GPU Process Memory(*)\\Dedicated Usage"),
                thermal: p.add("\\Thermal Zone Information(*)\\Temperature"),
            }
        } else {
            PdhIdx { mem_modified: None, mem_standby: [None; 3], mem_free: None, pages_in: None, pages_out: None, cpu_perf_total: None, cpu_freq_total: None, cpu_perf_each: None, ld_read: None, ld_write: None, ld_active: None, ld_lat: None, pd_read: None, pd_write: None, pd_active: None, net_rx: None, net_tx: None, net_bw: None, gpu_eng: None, gpu_mem: None, thermal: None }
        };
        let (gpu, gpu_err) = match Gpu::init() {
            Ok(g) => (Some(g), String::new()),
            Err(e) => (None, e),
        };
        let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let have_pdh_gpu = idx.gpu_eng.is_some();
        let c = Collector {
            pdh,
            idx,
            gpu,
            gpu_err,
            nproc_buf: Vec::new(),
            prev_procs: HashMap::new(),
            prev_ts: None,
            prev_cpu: Vec::new(),
            path_cache: HashMap::new(),
            cpu_name: cpu_brand(),
            logical,
            physical: physical_cores(),
            drives: Vec::new(),
            drives_at: std::time::Instant::now() - std::time::Duration::from_secs(3600),
            thermal_zones: Vec::new(),
            have_pdh_gpu,
        };
        // prime rate counters
        if let Some(p) = &c.pdh {
            p.collect();
        }
        c
    }

    pub fn gpu_name(&self) -> Option<(String, String)> {
        self.gpu.as_ref().map(|g| (g.name.clone(), g.driver.clone()))
    }

    pub fn cpu_name(&self) -> String {
        self.cpu_name.clone()
    }

    /// Collect everything for one tick.
    pub fn sample(&mut self, poll_ms: u64, want_gpu_procs: bool) -> (SystemSnap, Vec<ProcRow>) {
        let t0 = std::time::Instant::now();
        let now = std::time::Instant::now();
        let dt = self.prev_ts.map(|t| now.duration_since(t).as_secs_f64()).unwrap_or(0.0);
        self.prev_ts = Some(now);
        if let Some(p) = &self.pdh {
            p.collect();
        }

        // ---- CPU ----
        let times = winsys::query_cpu_times(self.logical);
        let mut per_core = Vec::with_capacity(times.len());
        let mut tot_busy = 0u64;
        let mut tot_all = 0u64;
        for (i, t) in times.iter().enumerate() {
            if let Some(p) = self.prev_cpu.get(i) {
                let dk = t.kernel.saturating_sub(p.kernel);
                let du = t.user.saturating_sub(p.user);
                let di = t.idle.saturating_sub(p.idle);
                let all = dk + du;
                let busy = all.saturating_sub(di);
                per_core.push(if all > 0 { busy as f64 * 100.0 / all as f64 } else { 0.0 });
                tot_busy += busy;
                tot_all += all;
            } else {
                per_core.push(0.0);
            }
        }
        self.prev_cpu = times;
        let cpu_total = if tot_all > 0 { tot_busy as f64 * 100.0 / tot_all as f64 } else { 0.0 };

        let mut cpu = CpuSnap { name: self.cpu_name.clone(), logical: self.logical, physical: self.physical, total: cpu_total, per_core, ..Default::default() };
        if self.pdh.is_some() {
            let perf = self.idx.cpu_perf_total.and_then(|i| self.pdh.as_ref().unwrap().value(i));
            let base = self.idx.cpu_freq_total.and_then(|i| self.pdh.as_ref().unwrap().value(i));
            cpu.base_mhz = base;
            if let (Some(p), Some(b)) = (perf, base) {
                cpu.freq_mhz = Some(b * p / 100.0);
            }
            if let Some(i) = self.idx.cpu_perf_each {
                let arr = self.pdh.as_mut().unwrap().array(i);
                let mut v: Vec<Option<f64>> = vec![None; self.logical];
                for (name, val) in arr {
                    if let Some((g, n)) = name.split_once(',') {
                        if let (Ok(g), Ok(n)) = (g.parse::<usize>(), n.parse::<usize>()) {
                            let k = g * 64 + n;
                            if k < v.len() {
                                v[k] = base.map(|b| b * val / 100.0);
                            }
                        }
                    }
                }
                cpu.per_core_freq = v;
            }
        }

        // ---- processes ----
        let raw = winsys::query_processes(&mut self.nproc_buf);
        let mut rows = Vec::with_capacity(raw.len());
        let mut next_prev: HashMap<(u32, u64), (u64, u64, u64)> = HashMap::with_capacity(raw.len());
        let ncpu = self.logical.max(1) as f64;

        // GPU per-process (PDH)
        let mut gpu_pct: HashMap<u32, f64> = HashMap::new();
        let mut gpu_mem: HashMap<u32, u64> = HashMap::new();
        let mut gpu_ok = false;
        if want_gpu_procs {
            if let Some(i) = self.idx.gpu_eng {
                let arr = self.pdh.as_mut().unwrap().array(i);
                if !arr.is_empty() {
                    gpu_ok = true;
                }
                // busiest single engine per process (same definition Task Manager uses); instances are per pid+adapter+engine
                for (name, v) in arr {
                    if let Some(pid) = parse_pid(&name) {
                        let e = gpu_pct.entry(pid).or_insert(0.0);
                        if v > *e {
                            *e = v.min(100.0);
                        }
                    }
                }
            }
            if let Some(i) = self.idx.gpu_mem {
                let arr = self.pdh.as_mut().unwrap().array(i);
                for (name, v) in arr {
                    if let Some(pid) = parse_pid(&name) {
                        *gpu_mem.entry(pid).or_insert(0) += v as u64;
                    }
                }
            }
        }

        let mut compressed: Option<u64> = None;
        for p in &raw {
            let key = (p.pid, p.create_time);
            let (cpu_pct, rbps, wbps) = match self.prev_procs.get(&key) {
                Some(&(pc, pr, pw)) if dt > 0.05 => (
                    p.cpu_time.saturating_sub(pc) as f64 / 1e7 / dt / ncpu * 100.0,
                    p.read_bytes.saturating_sub(pr) as f64 / dt,
                    p.write_bytes.saturating_sub(pw) as f64 / dt,
                ),
                _ => (0.0, 0.0, 0.0),
            };
            next_prev.insert(key, (p.cpu_time, p.read_bytes, p.write_bytes));
            if p.name == "Memory Compression" {
                compressed = Some(p.working_set);
            }
            let path = if p.pid <= 4 {
                String::new()
            } else {
                self.path_cache.entry(key).or_insert_with(|| winsys::process_path(p.pid).unwrap_or_default()).clone()
            };
            rows.push(row_from(p, path, cpu_pct.clamp(0.0, 100.0), rbps, wbps, gpu_pct.get(&p.pid).copied(), gpu_mem.get(&p.pid).copied(), gpu_ok));
        }
        self.prev_procs = next_prev;
        if self.path_cache.len() > raw.len() * 2 + 200 {
            let live: std::collections::HashSet<(u32, u64)> = rows.iter().map(|r| (r.pid, r.create_time)).collect();
            self.path_cache.retain(|k, _| live.contains(k));
        }

        // ---- memory ----
        let mi = winsys::query_memory();
        let ps = mi.page_size.max(4096);
        let pagefiles: Vec<PageFileSnap> = winsys::query_pagefiles(ps).into_iter().map(|p| PageFileSnap { name: p.name, total: p.total, in_use: p.in_use, peak: p.peak }).collect();
        let (mut modified, mut standby, mut free, mut pin, mut pout) = (None, None, None, None, None);
        if let Some(p) = &self.pdh {
            modified = self.idx.mem_modified.and_then(|i| p.value(i)).map(|v| v as u64);
            let parts: Vec<f64> = self.idx.mem_standby.iter().filter_map(|i| i.and_then(|i| p.value(i))).collect();
            if parts.len() == 3 {
                standby = Some(parts.iter().sum::<f64>() as u64);
            }
            free = self.idx.mem_free.and_then(|i| p.value(i)).map(|v| v as u64);
            pin = self.idx.pages_in.and_then(|i| p.value(i));
            pout = self.idx.pages_out.and_then(|i| p.value(i));
        }
        let in_use = mi.total.saturating_sub(mi.available).saturating_sub(modified.unwrap_or(0));
        let avail_pct = if mi.total > 0 { mi.available as f64 * 100.0 / mi.total as f64 } else { 100.0 };
        let commit_pct = if mi.commit_limit > 0 { mi.commit_total as f64 * 100.0 / mi.commit_limit as f64 } else { 0.0 };
        let (pressure, reason) = if avail_pct < 5.0 || commit_pct > 95.0 {
            ("critical", format!("Available memory {:.1}% of RAM, commit {:.0}% of limit", avail_pct, commit_pct))
        } else if avail_pct < 10.0 || commit_pct > 90.0 {
            ("high", format!("Available memory {:.1}% of RAM, commit {:.0}% of limit", avail_pct, commit_pct))
        } else if avail_pct < 20.0 || commit_pct > 80.0 {
            ("elevated", format!("Available memory {:.1}% of RAM, commit {:.0}% of limit", avail_pct, commit_pct))
        } else {
            ("normal", format!("{:.0}% of RAM is available (free + reusable cache); commit is {:.0}% of the limit", avail_pct, commit_pct))
        };
        let mem = MemSnap {
            total: mi.total,
            available: mi.available,
            in_use,
            modified,
            standby,
            free,
            compressed,
            commit_total: mi.commit_total,
            commit_limit: mi.commit_limit,
            commit_peak: mi.commit_peak,
            kernel_paged: mi.kernel_paged,
            kernel_nonpaged: mi.kernel_nonpaged,
            system_cache: mi.system_cache,
            pagefiles,
            pages_in_per_sec: pin,
            pages_out_per_sec: pout,
            handles: mi.handles,
            processes: mi.processes,
            threads: mi.threads,
            pressure: pressure.to_string(),
            pressure_reason: reason,
        };

        // ---- disks ----
        if self.drives_at.elapsed().as_secs() >= 5 {
            self.drives = winsys::drives();
            self.drives_at = std::time::Instant::now();
        }
        let mut ld_read: HashMap<String, f64> = HashMap::new();
        let mut ld_write: HashMap<String, f64> = HashMap::new();
        let mut ld_active: HashMap<String, f64> = HashMap::new();
        let mut ld_lat: HashMap<String, f64> = HashMap::new();
        let mut disk_io = DiskIoTotal::default();
        if let Some(p) = self.pdh.as_mut() {
            for (slot, map) in [(self.idx.ld_read, &mut ld_read), (self.idx.ld_write, &mut ld_write), (self.idx.ld_active, &mut ld_active), (self.idx.ld_lat, &mut ld_lat)] {
                if let Some(i) = slot {
                    for (n, v) in p.array(i) {
                        map.insert(n.to_ascii_uppercase(), v);
                    }
                }
            }
            disk_io.read_bps = self.idx.pd_read.and_then(|i| p.value(i));
            disk_io.write_bps = self.idx.pd_write.and_then(|i| p.value(i));
            disk_io.active_pct = self.idx.pd_active.and_then(|i| p.value(i)).map(|v| v.clamp(0.0, 100.0));
        }
        let disks: Vec<DiskSnap> = self.drives.clone()
            .into_iter()
            .map(|d| {
                let k = d.letter.to_ascii_uppercase();
                DiskSnap {
                    read_bps: ld_read.get(&k).copied(),
                    write_bps: ld_write.get(&k).copied(),
                    active_pct: ld_active.get(&k).map(|v| v.clamp(0.0, 100.0)),
                    latency_ms: ld_lat.get(&k).map(|v| v * 1000.0),
                    letter: d.letter,
                    label: d.label,
                    fs: d.fs,
                    kind: d.kind,
                    total: d.total,
                    free: d.free,
                }
            })
            .collect();

        // ---- network ----
        let mut net = NetSnap::default();
        if let Some(p) = self.pdh.as_mut() {
            let mut rx: HashMap<String, f64> = HashMap::new();
            let mut tx: HashMap<String, f64> = HashMap::new();
            let mut bw: HashMap<String, f64> = HashMap::new();
            for (slot, map) in [(self.idx.net_rx, &mut rx), (self.idx.net_tx, &mut tx), (self.idx.net_bw, &mut bw)] {
                if let Some(i) = slot {
                    for (n, v) in p.array(i) {
                        map.insert(n, v);
                    }
                }
            }
            let mut trx = 0.0;
            let mut ttx = 0.0;
            for (n, r) in &rx {
                let t = tx.get(n).copied().unwrap_or(0.0);
                let l = n.to_lowercase();
                // skip virtual/loopback pseudo-adapters so traffic isn't double counted
                if l.contains("loopback") || l.contains("isatap") || l.contains("teredo") {
                    continue;
                }
                trx += r;
                ttx += t;
                net.adapters.push(AdapterRate { name: n.clone(), rx_bps: *r, tx_bps: t, link_bps: bw.get(n).copied().unwrap_or(0.0) });
            }
            if !rx.is_empty() {
                net.rx_bps = Some(trx);
                net.tx_bps = Some(ttx);
            }
            net.adapters.sort_by(|a, b| (b.rx_bps + b.tx_bps).partial_cmp(&(a.rx_bps + a.tx_bps)).unwrap_or(std::cmp::Ordering::Equal));
        }

        // ---- thermal zones (ACPI) ----
        self.thermal_zones.clear();
        if let Some(i) = self.idx.thermal {
            for (n, v) in self.pdh.as_mut().unwrap().array(i) {
                if v > 200.0 && v < 450.0 {
                    self.thermal_zones.push((n, v - 273.15));
                }
            }
        }

        // ---- GPU ----
        let (gpu, gpu_status) = match &self.gpu {
            Some(g) => match g.read() {
                Some(s) => (Some(s), "ok".to_string()),
                None => (None, "NVML device read failed".to_string()),
            },
            None => (None, self.gpu_err.clone()),
        };

        let snap = SystemSnap {
            ts: now_ms(),
            uptime_s: winsys::uptime_secs(),
            cpu,
            cpu_temp: Reading::unavailable("pending"),
            cpu_power: Reading::unsupported("Package power is not exposed to unprivileged Windows APIs on this CPU"),
            mem,
            gpu,
            gpu_status,
            disks,
            disk_io,
            net,
            poll_ms,
            sample_cost_ms: t0.elapsed().as_secs_f64() * 1000.0,
        };
        (snap, rows)
    }
}

#[allow(clippy::too_many_arguments)]
fn row_from(p: &RawProc, path: String, cpu: f64, rbps: f64, wbps: f64, gpu: Option<f64>, gpu_mem: Option<u64>, gpu_known: bool) -> ProcRow {
    ProcRow {
        pid: p.pid,
        ppid: p.ppid,
        name: p.name.clone(),
        path,
        create_time: p.create_time,
        working_set: p.working_set,
        private_ws: p.private_ws,
        commit: p.commit,
        peak_ws: p.peak_ws,
        cpu,
        io_read_bps: rbps,
        io_write_bps: wbps,
        gpu: if gpu_known { Some(gpu.unwrap_or(0.0)) } else { None },
        gpu_mem: if gpu_known { Some(gpu_mem.unwrap_or(0)) } else { None },
        threads: p.threads,
        handles: p.handles,
        session: p.session,
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TopApp {
    pub name: String,
    pub count: u32,
    pub mem: u64,
    pub cpu: f64,
    pub gpu: f64,
    pub gpu_mem: u64,
}

/// Applications (all processes of one executable combined) ranked by memory, CPU and GPU; the union of the top 5 of each.
pub fn top_apps(rows: &[ProcRow]) -> Vec<TopApp> {
    let mut m: HashMap<String, TopApp> = HashMap::new();
    for p in rows {
        if p.pid == 0 {
            continue;
        }
        let e = m.entry(p.name.to_ascii_lowercase()).or_insert(TopApp { name: p.name.clone(), count: 0, mem: 0, cpu: 0.0, gpu: 0.0, gpu_mem: 0 });
        e.count += 1;
        e.mem += p.private_ws;
        e.cpu += p.cpu;
        e.gpu += p.gpu.unwrap_or(0.0);
        e.gpu_mem += p.gpu_mem.unwrap_or(0);
    }
    let all: Vec<TopApp> = m.into_values().collect();
    let mut keep: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut idx: Vec<usize> = (0..all.len()).collect();
    idx.sort_by(|&a, &b| all[b].mem.cmp(&all[a].mem));
    keep.extend(idx.iter().take(5));
    idx.sort_by(|&a, &b| all[b].cpu.partial_cmp(&all[a].cpu).unwrap_or(std::cmp::Ordering::Equal));
    keep.extend(idx.iter().take(5));
    idx.sort_by(|&a, &b| all[b].gpu.partial_cmp(&all[a].gpu).unwrap_or(std::cmp::Ordering::Equal).then(all[b].gpu_mem.cmp(&all[a].gpu_mem)));
    keep.extend(idx.iter().take(5));
    all.into_iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, a)| a).collect()
}

/// FILETIME values exceed 2^53, so they cross to JavaScript as strings to stay exact.
fn ser_u64_string<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn parse_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    rest.split('_').next()?.parse().ok()
}

