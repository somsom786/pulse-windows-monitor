use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub poll_interval_ms: u64,
    pub record_enabled: bool,
    pub record_interval_s: u64,
    pub proc_snapshot_interval_s: u64,
    pub retention_days: u32,
    pub monitor_in_background: bool,
    pub minimize_to_tray: bool,
    pub temp_unit: String, // "C" | "F"
    pub theme: String,     // "dark" | "light" | "system"
    pub notifications: bool,
    pub alert_mem_enabled: bool,
    pub alert_mem_pct: f64,
    pub alert_mem_sustain_s: u64,
    pub alert_disk_enabled: bool,
    pub alert_disk_free_pct: f64,
    pub alert_gpu_temp_enabled: bool,
    pub alert_gpu_temp_c: f64,
    pub alert_cpu_enabled: bool,
    pub alert_cpu_pct: f64,
    pub alert_cpu_sustain_s: u64,
    pub alert_cooldown_min: u64,
    pub spike_mem_gb: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            poll_interval_ms: 2000,
            record_enabled: true,
            record_interval_s: 10,
            proc_snapshot_interval_s: 15,
            retention_days: 7,
            monitor_in_background: true,
            minimize_to_tray: true,
            temp_unit: "C".into(),
            theme: "dark".into(),
            notifications: true,
            alert_mem_enabled: true,
            alert_mem_pct: 92.0,
            alert_mem_sustain_s: 30,
            alert_disk_enabled: true,
            alert_disk_free_pct: 10.0,
            alert_gpu_temp_enabled: true,
            alert_gpu_temp_c: 85.0,
            alert_cpu_enabled: false,
            alert_cpu_pct: 95.0,
            alert_cpu_sustain_s: 60,
            alert_cooldown_min: 15,
            spike_mem_gb: 4.0,
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("PULSE");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(data_dir().join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default()
    }

    pub fn sanitized(mut self) -> Settings {
        self.poll_interval_ms = self.poll_interval_ms.clamp(500, 30_000);
        self.record_interval_s = self.record_interval_s.clamp(2, 300);
        self.proc_snapshot_interval_s = self.proc_snapshot_interval_s.clamp(5, 600);
        self.retention_days = self.retention_days.clamp(1, 90);
        self.alert_mem_pct = self.alert_mem_pct.clamp(50.0, 99.9);
        self.alert_cooldown_min = self.alert_cooldown_min.clamp(1, 1440);
        self
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(data_dir().join("settings.json"), s);
        }
    }
}
