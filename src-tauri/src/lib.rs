pub mod actions;
pub mod analysis;
pub mod categories;
pub mod gpu;
pub mod monitor;
pub mod pdh;
pub mod recorder;
pub mod scanner;
pub mod sensors;
pub mod settings;
pub mod store;
pub mod winsys;

mod commands;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

pub fn fmt_bytes(b: u64) -> String {
    recorder::fmt_b(b as f64)
}

/// RFC 3339 UTC timestamp from unix milliseconds.
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

pub struct AppState {
    pub settings: RwLock<settings::Settings>,
    pub store: Arc<store::Store>,
    pub latest: Mutex<Option<monitor::SystemSnap>>,
    pub latest_procs: Mutex<Vec<monitor::ProcRow>>,
    pub want_procs: AtomicBool,
    pub want_top: AtomicBool,
    pub paused: AtomicBool,
    pub visible: AtomicBool,
    pub hub: Arc<sensors::SensorHub>,
    pub tree: Mutex<Option<(String, Arc<scanner::ScanTree>, Instant)>>,
    pub current_scan: Mutex<Option<String>>,
    pub overview: Mutex<Option<(String, analysis::Overview)>>,
    pub scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub scanning: AtomicBool,
    pub dup_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub dup_result: Mutex<Option<Vec<analysis::DupGroup>>>,
    pub inv: Mutex<sensors::InvCache>,
    pub wake: Sender<()>,
    pub icons: Mutex<HashMap<String, Option<String>>>,
}

impl AppState {
    /// The currently selected scan, loaded from disk if it was unloaded to save memory.
    pub fn get_tree(&self) -> Result<Arc<scanner::ScanTree>, String> {
        let mut g = self.tree.lock();
        if let Some((_, t, used)) = g.as_mut() {
            *used = Instant::now();
            return Ok(t.clone());
        }
        let id = self.current_scan.lock().clone().ok_or_else(|| "No scan is open. Run a scan first.".to_string())?;
        let t = Arc::new(scanner::load_tree(&id)?);
        *g = Some((id, t.clone(), Instant::now()));
        Ok(t)
    }
}

fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

fn monitor_loop(app: tauri::AppHandle, st: Arc<AppState>, rx: Receiver<()>) {
    let mut col = monitor::Collector::new();
    let mut rec = recorder::Recorder::new(st.store.clone());
    let mut snap_count = 0u64;
    loop {
        let s = st.settings.read().clone();
        let active = !st.paused.load(Ordering::Relaxed) && (st.visible.load(Ordering::Relaxed) || s.monitor_in_background);
        let mut wait = Duration::from_millis(s.poll_interval_ms);
        if active {
            let (mut snap, rows) = col.sample(s.poll_interval_ms, true);
            // CPU temperature / power come only from a real sensor provider
            let hub = st.hub.snapshot();
            snap.cpu_temp = match st.hub.cpu_temp() {
                Some((v, src)) => gpu::Reading { value: Some(v), status: "ok", note: Some(src) },
                None if hub.status == "ok" => gpu::Reading::unsupported("The sensor provider reported no CPU temperature sensor"),
                None => gpu::Reading::needs_admin("CPU temperature on this CPU needs a kernel sensor driver. Start LibreHardwareMonitor as administrator and PULSE will read it."),
            };
            if let Some((v, src)) = st.hub.cpu_power() {
                snap.cpu_power = gpu::Reading { value: Some(v), status: "ok", note: Some(src) };
            }
            let events = rec.on_tick(&snap, &rows, &s);
            for e in events {
                match e {
                    recorder::Event::Alert { kind, title, body, .. } => {
                        if s.notifications {
                            notify(&app, &title, &body);
                        }
                        let _ = app.emit("alert", serde_json::json!({"kind": kind, "title": title, "body": body}));
                    }
                    recorder::Event::Spike { kind, summary } => {
                        let _ = app.emit("spike", serde_json::json!({"kind": kind, "summary": summary}));
                    }
                }
            }
            // nothing is pushed to the UI while the window is hidden; recording continues regardless
            if st.visible.load(Ordering::Relaxed) {
                let _ = app.emit("telemetry", &snap);
                if st.want_procs.load(Ordering::Relaxed) {
                    let _ = app.emit("processes", &rows);
                } else if st.want_top.load(Ordering::Relaxed) {
                    let _ = app.emit("top-apps", monitor::top_apps(&rows));
                }
            }
            *st.latest.lock() = Some(snap);
            *st.latest_procs.lock() = rows;
            snap_count += 1;
            // release the scan tree after a few idle minutes: it can be hundreds of MB
            if snap_count % 10 == 0 {
                let mut g = st.tree.lock();
                if let Some((_, _, used)) = g.as_ref() {
                    if used.elapsed() > Duration::from_secs(300) && !st.scanning.load(Ordering::Relaxed) {
                        *g = None;
                    }
                }
            }
        } else {
            wait = Duration::from_millis(500);
        }
        let _ = rx.recv_timeout(wait);
    }
}

/// Show the main window, creating it if it was destroyed to free the WebView2 processes while in the tray.
fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let Some(cfg) = app.config().app.windows.first().cloned() else { return };
    match tauri::WebviewWindowBuilder::from_config(app, &cfg).and_then(|b| b.build()) {
        Ok(w) => {
            if let Some(st) = app.try_state::<Arc<AppState>>() {
                st.visible.store(true, Ordering::Relaxed);
                let _ = st.wake.send(());
            }
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("could not create window: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = settings::Settings::load().sanitized();
    let store = Arc::new(store::Store::open().expect("cannot open history database"));
    let (wake_tx, wake_rx) = crossbeam_channel::unbounded::<()>();
    let hub = sensors::SensorHub::start();
    let state = Arc::new(AppState {
        settings: RwLock::new(settings),
        store,
        latest: Mutex::new(None),
        latest_procs: Mutex::new(Vec::new()),
        want_procs: AtomicBool::new(false),
        want_top: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        visible: AtomicBool::new(true),
        hub,
        tree: Mutex::new(None),
        current_scan: Mutex::new(None),
        overview: Mutex::new(None),
        scan_cancel: Mutex::new(None),
        scanning: AtomicBool::new(false),
        dup_cancel: Mutex::new(None),
        dup_result: Mutex::new(None),
        inv: Mutex::new(sensors::InvCache { at: Instant::now(), data: None }),
        wake: wake_tx,
        icons: Mutex::new(HashMap::new()),
    });

    let st_setup = state.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .manage(state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            // tray
            let show = tauri::menu::MenuItem::with_id(app, "show", "Show PULSE", true, None::<&str>)?;
            let pause = tauri::menu::MenuItem::with_id(app, "pause", "Pause / resume monitoring", true, None::<&str>)?;
            let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit PULSE", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show, &pause, &quit])?;
            let st_tray = st_setup.clone();
            let mut tb = tauri::tray::TrayIconBuilder::with_id("main")
                .tooltip("PULSE")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id.as_ref() {
                    "show" => show_main(app),
                    "pause" => {
                        let v = !st_tray.paused.load(Ordering::Relaxed);
                        st_tray.paused.store(v, Ordering::Relaxed);
                        let _ = st_tray.wake.send(());
                        let _ = app.emit("paused", v);
                    }
                    "quit" => {
                        st_tray.store.flush();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = ev {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tb = tb.icon(icon.clone());
            }
            tb.build(app)?;

            // started minimized (launch at sign-in): no window at all until the tray icon is used
            if std::env::args().any(|a| a == "--minimized") {
                st_setup.visible.store(false, Ordering::Relaxed);
            } else {
                show_main(app.handle());
            }
            let st_m = st_setup.clone();
            std::thread::Builder::new().name("pulse-monitor".into()).spawn(move || monitor_loop(handle, st_m, wake_rx)).expect("monitor thread");
            Ok(())
        })
        .on_window_event(|window, event| {
            let st = window.app_handle().state::<Arc<AppState>>();
            match event {
                // closing to the tray destroys the window (and its WebView2 processes, ~400 MB); the app keeps monitoring
                tauri::WindowEvent::Destroyed => {
                    st.visible.store(false, Ordering::Relaxed);
                    st.want_procs.store(false, Ordering::Relaxed);
                    st.want_top.store(false, Ordering::Relaxed);
                }
                tauri::WindowEvent::Focused(true) => {
                    st.visible.store(true, Ordering::Relaxed);
                    let _ = st.wake.send(());
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::get_paused,
            commands::set_paused,
            commands::set_view,
            commands::get_snapshot,
            commands::get_processes,
            commands::process_icon,
            commands::terminate_process,
            commands::history_series,
            commands::history_snap_at,
            commands::history_spikes,
            commands::history_peaks,
            commands::history_app,
            commands::history_growth,
            commands::history_alerts,
            commands::store_info,
            commands::clear_history,
            commands::export_csv,
            commands::attention,
            commands::drives_list,
            commands::scan_start,
            commands::scan_cancel,
            commands::scan_list,
            commands::scan_open,
            commands::scan_delete,
            commands::scan_state,
            commands::tree_children,
            commands::tree_treemap,
            commands::tree_crumbs,
            commands::tree_find,
            commands::tree_search,
            commands::storage_overview,
            commands::storage_locations,
            commands::storage_loc_children,
            commands::storage_installers,
            commands::storage_apps,
            commands::storage_diff,
            commands::dup_start,
            commands::dup_cancel,
            commands::dup_result,
            commands::act_reveal,
            commands::act_open,
            commands::act_props,
            commands::act_recycle,
            commands::act_system,
            commands::act_protected,
            commands::hardware_info,
            commands::sensor_hub,
            commands::autostart_get,
            commands::autostart_set,
            commands::app_info,
            commands::restart_elevated,
            commands::lhm_info,
            commands::lhm_launch,
        ])
        .build(tauri::generate_context!())
        .expect("error while building PULSE")
        .run(|app, event| match event {
            // last window closed: stay alive in the tray unless the user disabled that
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() {
                    let keep = app.try_state::<Arc<AppState>>().map(|st| st.settings.read().minimize_to_tray).unwrap_or(false);
                    if keep {
                        api.prevent_exit();
                    }
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(st) = app.try_state::<Arc<AppState>>() {
                    st.store.flush();
                }
            }
            _ => {}
        });
}
