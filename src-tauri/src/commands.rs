//! Tauri command surface. All heavy work runs off the UI thread (`async` commands use the runtime pool).

use crate::analysis::{self, Filter};
use crate::monitor::{now_ms, ProcRow, SystemSnap};
use crate::scanner;
use crate::settings::Settings;
use crate::store::*;
use crate::{actions, AppState};
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, State};

type St<'a> = State<'a, Arc<AppState>>;
type R<T> = Result<T, String>;

// ---------------- settings / control ----------------

#[tauri::command(async)]
pub fn get_settings(st: St) -> Settings {
    st.settings.read().clone()
}

#[tauri::command(async)]
pub fn set_settings(st: St, settings: Settings) -> R<Settings> {
    let s = settings.sanitized();
    s.save();
    *st.settings.write() = s.clone();
    let _ = st.wake.send(());
    Ok(s)
}

#[tauri::command(async)]
pub fn get_paused(st: St) -> bool {
    st.paused.load(Ordering::Relaxed)
}

#[tauri::command(async)]
pub fn set_paused(st: St, app: tauri::AppHandle, paused: bool) {
    st.paused.store(paused, Ordering::Relaxed);
    let _ = st.wake.send(());
    let _ = app.emit("paused", paused);
}

/// The frontend tells the backend which view is open so the (large) process table is only streamed when needed.
#[tauri::command(async)]
pub fn set_view(st: St, view: String) {
    st.want_procs.store(view == "processes", Ordering::Relaxed);
    st.want_top.store(view == "overview", Ordering::Relaxed);
    st.visible.store(true, Ordering::Relaxed);
}

#[tauri::command(async)]
pub fn get_snapshot(st: St) -> Option<SystemSnap> {
    st.latest.lock().clone()
}

#[tauri::command(async)]
pub fn get_processes(st: St) -> Vec<ProcRow> {
    st.latest_procs.lock().clone()
}

#[tauri::command(async)]
pub fn process_icon(st: St, path: String) -> Option<String> {
    if let Some(v) = st.icons.lock().get(&path) {
        return v.clone();
    }
    let v = actions::icon_png_base64(&path);
    st.icons.lock().insert(path, v.clone());
    v
}

#[tauri::command(async)]
pub fn terminate_process(st: St, pid: u32, create_time: String, name: String) -> R<()> {
    actions::can_terminate(&name, pid)?;
    let ct: u64 = create_time.parse().map_err(|_| "Invalid process identity".to_string())?;
    // the name must still match what the user saw, and the OS handle's creation time is verified before terminating
    let procs = st.latest_procs.lock();
    let known = procs.iter().find(|p| p.pid == pid && p.create_time == ct).map(|p| p.name.eq_ignore_ascii_case(&name));
    drop(procs);
    match known {
        Some(true) => crate::winsys::terminate_checked(pid, ct),
        Some(false) => Err("That PID now belongs to a different process; refresh and try again.".into()),
        None => Err("Process has already exited or changed; refresh and try again.".into()),
    }
}

// ---------------- history ----------------

#[tauri::command(async)]
pub fn history_series(st: St, from: i64, to: i64, max_points: Option<i64>) -> Vec<Point> {
    let n = max_points.unwrap_or(600).clamp(50, 3000);
    let bucket = ((to - from) / n).max(st.settings.read().record_interval_s as i64 * 1000);
    st.store.series(from, to, bucket)
}

#[tauri::command(async)]
pub fn history_snap_at(st: St, ts: i64) -> Option<SnapAt> {
    let tol = (st.settings.read().proc_snapshot_interval_s as i64 * 2 + 10) * 1000;
    st.store.snap_at(ts, tol.max(40_000))
}

#[tauri::command(async)]
pub fn history_spikes(st: St, from: i64, to: i64, limit: Option<i64>) -> Vec<SpikeRow> {
    st.store.spikes(from, to, limit.unwrap_or(200))
}

#[tauri::command(async)]
pub fn history_peaks(st: St, from: i64, to: i64, limit: Option<i64>) -> Vec<PeakRow> {
    st.store.peaks(from, to, limit.unwrap_or(25))
}

#[tauri::command(async)]
pub fn history_app(st: St, name: String, from: i64, to: i64) -> Vec<(i64, u64)> {
    st.store.app_history(&name, from, to)
}

#[tauri::command(async)]
pub fn history_growth(st: St, window_min: Option<i64>) -> Vec<GrowthRow> {
    st.store.growth(window_min.unwrap_or(60))
}

#[tauri::command(async)]
pub fn history_alerts(st: St) -> Vec<AlertRow> {
    st.store.alerts(100)
}

#[tauri::command(async)]
pub fn store_info(st: St) -> StoreInfo {
    st.store.info()
}

#[tauri::command(async)]
pub fn clear_history(st: St) {
    st.store.clear();
}

#[tauri::command(async)]
pub fn export_csv(st: St, from: i64, to: i64, path: String) -> R<usize> {
    st.store.export_csv(from, to, &path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    severity: &'static str,
    title: String,
    detail: String,
    ts: Option<i64>,
    view: Option<&'static str>,
}

#[tauri::command(async)]
pub fn attention(st: St) -> Vec<Attention> {
    use crate::recorder::fmt_b;
    let mut out = Vec::new();
    let now = now_ms() as i64;
    if let Some(s) = st.latest.lock().clone() {
        for d in &s.disks {
            if d.kind == "fixed" && d.total > 0 {
                let pct = d.free as f64 * 100.0 / d.total as f64;
                if pct < 10.0 {
                    out.push(Attention { severity: if pct < 5.0 { "crit" } else { "warn" }, title: format!("{} drive has less than 10% free space", d.letter), detail: format!("{} free of {} ({:.1}%).", fmt_b(d.free as f64), fmt_b(d.total as f64), pct), ts: None, view: Some("storage") });
                }
            }
        }
        if s.mem.pressure == "high" || s.mem.pressure == "critical" {
            out.push(Attention { severity: if s.mem.pressure == "critical" { "crit" } else { "warn" }, title: format!("Memory pressure is {}", s.mem.pressure), detail: s.mem.pressure_reason.clone(), ts: None, view: Some("processes") });
        }
        if let Some(g) = &s.gpu {
            if let (Some(t), Some(th)) = (g.temp.value, g.temp_slowdown.value) {
                if t >= th - 3.0 {
                    out.push(Attention { severity: "warn", title: format!("GPU temperature is {:.0}°C", t), detail: format!("This is within 3°C of the GPU's slowdown threshold ({:.0}°C).", th), ts: None, view: Some("hardware") });
                }
            }
            if !g.throttle.is_empty() && g.throttle.iter().any(|t| t.contains("thermal")) {
                out.push(Attention { severity: "warn", title: "GPU is thermally throttling".into(), detail: g.throttle.join(", "), ts: None, view: Some("hardware") });
            }
        }
    }
    let info = st.store.info();
    let rec_from = info.oldest_ts.unwrap_or(now);
    let hour_from = (now - 3_600_000).max(rec_from);
    let span_min = ((now - hour_from) / 60_000).max(0);
    if let Some(p) = st.store.peaks(hour_from, now, 1).into_iter().next() {
        out.push(Attention {
            severity: "info",
            title: format!("{} had the highest observed memory use in the last {}", p.name, if span_min >= 59 { "hour".to_string() } else { format!("{} minutes recorded", span_min) }),
            detail: format!("{} private memory{} (summed across its processes) was the highest group in recorded snapshots.", fmt_b(p.peak_private_ws as f64), if p.max_instances > 1 { format!(" across {} processes", p.max_instances) } else { String::new() }),
            ts: Some(p.peak_ts),
            view: Some("processes"),
        });
    }
    let series = st.store.series(now - 24 * 3_600_000, now, 60_000);
    if let Some(pt) = series.iter().filter(|p| p.gpu_temp.is_some()).max_by(|a, b| a.gpu_temp.partial_cmp(&b.gpu_temp).unwrap_or(std::cmp::Ordering::Equal)) {
        out.push(Attention { severity: "info", title: format!("GPU temperature peaked at {:.0}°C in recorded history", pt.gpu_temp.unwrap()), detail: "Highest GPU temperature in the last 24 hours of recorded data.".into(), ts: Some(pt.t), view: Some("history") });
    }
    for g in st.store.growth(60).into_iter().take(3) {
        out.push(Attention { severity: "warn", title: format!("Sustained memory growth observed in {}", g.name), detail: format!("Private memory grew from {} to {} over {:.0} minutes (about {} per hour). This does not by itself indicate a leak.", fmt_b(g.start_ws as f64), fmt_b(g.now_ws as f64), g.minutes, fmt_b(g.growth_per_hour)), ts: None, view: Some("processes") });
    }
    if let Some(sp) = st.store.spikes(now - 24 * 3_600_000, now, 1).into_iter().next() {
        out.push(Attention { severity: "info", title: format!("Most recent {} spike", sp.kind), detail: sp.summary, ts: Some(sp.ts), view: Some("history") });
    }
    if scanner::list_meta().is_empty() {
        out.push(Attention { severity: "info", title: "No storage scan has been run yet".into(), detail: "Run a scan in Storage Explorer to see what is using disk space. PULSE never scans automatically.".into(), ts: None, view: Some("storage") });
    }
    out
}

// ---------------- storage: scanning ----------------

#[tauri::command(async)]
pub fn drives_list() -> Vec<crate::winsys::Drive> {
    crate::winsys::drives()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScanDone {
    meta: scanner::ScanMeta,
    cancelled: bool,
}

#[tauri::command(async)]
pub fn scan_start(st: St, app: tauri::AppHandle, roots: Vec<String>) -> R<()> {
    if st.scanning.swap(true, Ordering::SeqCst) {
        return Err("A scan is already running".into());
    }
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *st.scan_cancel.lock() = Some(cancel.clone());
    let state: Arc<AppState> = st.inner().clone();
    // release any loaded tree to reduce peak memory
    *state.tree.lock() = None;
    std::thread::Builder::new()
        .name("pulse-scan".into())
        .spawn(move || {
            let total = roots.len();
            for (i, root) in roots.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let _ = app.emit("scan-start", serde_json::json!({"root": root, "index": i, "total": total}));
                let a = app.clone();
                let tree = scanner::scan(root, cancel.clone(), move |p| {
                    let _ = a.emit("scan-progress", p);
                });
                if tree.meta.cancelled {
                    let _ = app.emit("scan-done", ScanDone { meta: tree.meta.clone(), cancelled: true });
                    break;
                }
                let _ = app.emit("scan-saving", &tree.meta);
                if let Err(e) = scanner::save_tree(&tree) {
                    let _ = app.emit("scan-error", format!("Could not save scan: {}", e));
                }
                let meta = tree.meta.clone();
                *state.current_scan.lock() = Some(meta.id.clone());
                *state.overview.lock() = None;
                *state.tree.lock() = Some((meta.id.clone(), Arc::new(tree), std::time::Instant::now()));
                let _ = app.emit("scan-done", ScanDone { meta, cancelled: false });
            }
            state.scanning.store(false, Ordering::SeqCst);
            *state.scan_cancel.lock() = None;
            let _ = app.emit("scan-finished", ());
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn scan_cancel(st: St) {
    if let Some(c) = st.scan_cancel.lock().as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanState {
    scanning: bool,
    current: Option<String>,
    loaded: bool,
}

#[tauri::command(async)]
pub fn scan_state(st: St) -> ScanState {
    ScanState { scanning: st.scanning.load(Ordering::Relaxed), current: st.current_scan.lock().clone(), loaded: st.tree.lock().is_some() }
}

#[tauri::command(async)]
pub fn scan_list() -> Vec<scanner::ScanMeta> {
    scanner::list_meta()
}

#[tauri::command(async)]
pub fn scan_open(st: St, id: String) -> R<scanner::ScanMeta> {
    let t = scanner::load_tree(&id)?;
    let meta = t.meta.clone();
    *st.current_scan.lock() = Some(id.clone());
    *st.overview.lock() = None;
    *st.tree.lock() = Some((id, Arc::new(t), std::time::Instant::now()));
    Ok(meta)
}

#[tauri::command(async)]
pub fn scan_delete(st: St, id: String) {
    scanner::delete_scan(&id);
    let mut cur = st.current_scan.lock();
    if cur.as_deref() == Some(id.as_str()) {
        *cur = None;
        *st.tree.lock() = None;
        *st.overview.lock() = None;
    }
}

// ---------------- storage: queries ----------------

#[tauri::command(async)]
pub fn tree_children(st: St, node: u32, metric: String, limit: Option<usize>) -> R<analysis::Children> {
    let t = st.get_tree()?;
    if node as usize >= t.nodes.len() {
        return Err("Unknown node".into());
    }
    Ok(analysis::children(&t, node, &metric, limit.unwrap_or(500)))
}

#[tauri::command(async)]
pub fn tree_treemap(st: St, node: u32, metric: String, depth: Option<u32>) -> R<analysis::Tm> {
    let t = st.get_tree()?;
    if node as usize >= t.nodes.len() {
        return Err("Unknown node".into());
    }
    Ok(analysis::treemap(&t, node, &metric, depth.unwrap_or(2)))
}

#[tauri::command(async)]
pub fn tree_crumbs(st: St, node: u32) -> R<Vec<analysis::Crumb>> {
    let t = st.get_tree()?;
    if node as usize >= t.nodes.len() {
        return Err("Unknown node".into());
    }
    Ok(analysis::breadcrumbs(&t, node))
}

#[tauri::command(async)]
pub fn tree_find(st: St, path: String) -> R<Option<u32>> {
    let t = st.get_tree()?;
    Ok(t.find_path(&path))
}

#[tauri::command(async)]
pub fn tree_search(st: St, filter: Filter) -> R<analysis::SearchResult> {
    let t = st.get_tree()?;
    Ok(analysis::search(&t, &filter))
}

#[tauri::command(async)]
pub fn storage_overview(st: St) -> R<analysis::Overview> {
    let t = st.get_tree()?;
    {
        let g = st.overview.lock();
        if let Some((id, o)) = g.as_ref() {
            if *id == t.meta.id {
                return Ok(o.clone());
            }
        }
    }
    let o = analysis::overview(&t);
    *st.overview.lock() = Some((t.meta.id.clone(), o.clone()));
    Ok(o)
}

#[tauri::command(async)]
pub fn storage_locations(st: St) -> R<Vec<analysis::Location>> {
    let t = st.get_tree()?;
    Ok(analysis::locations(&t))
}

#[tauri::command(async)]
pub fn storage_loc_children(st: St, path: String) -> R<Vec<analysis::Row>> {
    let t = st.get_tree()?;
    Ok(analysis::top_children_of(&t, &path, 25))
}

#[tauri::command(async)]
pub fn storage_installers(st: St) -> R<Vec<analysis::Row>> {
    let t = st.get_tree()?;
    Ok(analysis::old_installers(&t))
}

#[tauri::command(async)]
pub fn storage_apps(st: St) -> Vec<analysis::App> {
    let t = st.get_tree().ok();
    analysis::installed_apps(t.as_deref())
}

#[tauri::command(async)]
pub fn storage_diff(a: String, b: String) -> R<analysis::Diff> {
    let ta = scanner::load_tree(&a)?;
    let tb = scanner::load_tree(&b)?;
    Ok(analysis::diff(&ta, &tb))
}

#[tauri::command(async)]
pub fn dup_start(st: St, app: tauri::AppHandle, min_size_mb: Option<u64>) -> R<()> {
    let t = st.get_tree()?;
    if st.dup_cancel.lock().is_some() {
        return Err("A duplicate scan is already running".into());
    }
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    *st.dup_cancel.lock() = Some(cancel.clone());
    *st.dup_result.lock() = None;
    let state: Arc<AppState> = st.inner().clone();
    let min = min_size_mb.unwrap_or(1).max(1) * 1024 * 1024;
    std::thread::Builder::new()
        .name("pulse-dups".into())
        .spawn(move || {
            let a = app.clone();
            let res = analysis::find_duplicates(&t, min, &cancel, move |p| {
                let _ = a.emit("dup-progress", p);
            });
            let cancelled = cancel.load(Ordering::Relaxed);
            if !cancelled {
                *state.dup_result.lock() = Some(res);
            }
            *state.dup_cancel.lock() = None;
            let _ = app.emit("dup-done", cancelled);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn dup_cancel(st: St) {
    if let Some(c) = st.dup_cancel.lock().as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

#[tauri::command(async)]
pub fn dup_result(st: St) -> Option<Vec<analysis::DupGroup>> {
    st.dup_result.lock().clone()
}

// ---------------- actions ----------------

#[tauri::command(async)]
pub fn act_reveal(path: String) -> R<()> {
    actions::reveal_in_explorer(&path)
}

#[tauri::command(async)]
pub fn act_open(path: String) -> R<()> {
    actions::open_path(&path)
}

#[tauri::command(async)]
pub fn act_props(path: String) -> R<()> {
    actions::show_properties(&path)
}

#[tauri::command(async)]
pub fn act_recycle(paths: Vec<String>) -> R<usize> {
    actions::recycle(&paths)
}

#[tauri::command(async)]
pub fn act_system(target: String) -> R<()> {
    actions::open_uri(&target)
}

#[tauri::command(async)]
pub fn act_protected(path: String) -> Option<String> {
    actions::is_protected(&path).map(|s| s.to_string())
}

// ---------------- hardware ----------------

#[tauri::command(async)]
pub fn hardware_info(st: St) -> crate::sensors::Inventory {
    {
        let g = st.inv.lock();
        if let Some(d) = &g.data {
            if g.at.elapsed().as_secs() < 60 {
                return d.clone();
            }
        }
    }
    let inv = crate::sensors::inventory();
    let mut g = st.inv.lock();
    g.at = std::time::Instant::now();
    g.data = Some(inv.clone());
    inv
}

#[tauri::command(async)]
pub fn sensor_hub(st: St) -> crate::sensors::HubData {
    st.hub.snapshot()
}

#[tauri::command(async)]
pub fn autostart_get(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command(async)]
pub fn autostart_set(app: tauri::AppHandle, enabled: bool) -> R<()> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    if enabled { m.enable() } else { m.disable() }.map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    data_dir: String,
    is_admin: bool,
    pid: u32,
    cpu_name: String,
}

#[tauri::command(async)]
pub fn app_info(st: St) -> AppInfo {
    let _ = &st;
    AppInfo { version: env!("CARGO_PKG_VERSION"), data_dir: crate::settings::data_dir().to_string_lossy().into_owned(), is_admin: is_elevated(), pid: std::process::id(), cpu_name: String::new() }
}

fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::*;
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut elev as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut ret).is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LhmInfo {
    installed: bool,
    path: String,
}

#[tauri::command(async)]
pub fn lhm_info() -> LhmInfo {
    let p = actions::lhm_path();
    LhmInfo { installed: p.is_some(), path: p.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default() }
}

#[tauri::command(async)]
pub fn lhm_launch() -> R<()> {
    let p = actions::lhm_path().ok_or_else(|| "LibreHardwareMonitor is not installed in PULSE's tools folder".to_string())?;
    actions::launch_elevated(&p)
}

#[tauri::command(async)]
pub fn restart_elevated(st: St, app: tauri::AppHandle) -> R<()> {
    st.store.flush();
    actions::relaunch_elevated()?;
    app.exit(0);
    Ok(())
}
