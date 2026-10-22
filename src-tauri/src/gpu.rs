//! NVIDIA telemetry through NVML. Every field is optional and carries an explicit status.

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor, TemperatureThreshold};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::Nvml;
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub value: Option<f64>,
    pub status: &'static str, // ok | unsupported | needs_admin | unavailable
    pub note: Option<String>,
}

impl Reading {
    pub fn ok(v: f64) -> Reading {
        Reading { value: Some(v), status: "ok", note: None }
    }
    pub fn unsupported(note: &str) -> Reading {
        Reading { value: None, status: "unsupported", note: Some(note.to_string()) }
    }
    pub fn needs_admin(note: &str) -> Reading {
        Reading { value: None, status: "needs_admin", note: Some(note.to_string()) }
    }
    pub fn unavailable(note: &str) -> Reading {
        Reading { value: None, status: "unavailable", note: Some(note.to_string()) }
    }
}

fn rd<T>(r: Result<T, NvmlError>, f: impl FnOnce(T) -> f64) -> Reading {
    match r {
        Ok(v) => Reading::ok(f(v)),
        Err(NvmlError::NotSupported) => Reading::unsupported("Not supported by this GPU or driver"),
        Err(NvmlError::NoPermission) => Reading::needs_admin("Requires administrator privileges"),
        Err(e) => Reading::unavailable(&format!("{}", e)),
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GpuSnap {
    pub name: String,
    pub driver: String,
    pub util: Reading,
    pub mem_ctrl_util: Reading,
    pub vram_used: Reading,
    pub vram_total: Reading,
    pub temp: Reading,
    pub temp_slowdown: Reading,
    pub hotspot: Reading,
    pub fan_pct: Reading,
    pub fan_rpm: Reading,
    pub power_w: Reading,
    pub power_limit_w: Reading,
    pub clock_gpu: Reading,
    pub clock_mem: Reading,
    pub clock_gpu_max: Reading,
    pub clock_mem_max: Reading,
    pub pstate: Option<String>,
    pub throttle: Vec<String>,
    pub throttle_note: String,
}

pub struct Gpu {
    nvml: Nvml,
    pub name: String,
    pub driver: String,
}

impl Gpu {
    pub fn init() -> Result<Gpu, String> {
        let nvml = Nvml::init().map_err(|e| format!("NVML unavailable: {}", e))?;
        let count = nvml.device_count().map_err(|e| e.to_string())?;
        if count == 0 {
            return Err("No NVIDIA GPU found".into());
        }
        let dev = nvml.device_by_index(0).map_err(|e| e.to_string())?;
        let name = dev.name().unwrap_or_else(|_| "NVIDIA GPU".into());
        let driver = nvml.sys_driver_version().unwrap_or_default();
        drop(dev);
        Ok(Gpu { nvml, name, driver })
    }

    pub fn read(&self) -> Option<GpuSnap> {
        let d = self.nvml.device_by_index(0).ok()?;
        let util = d.utilization_rates();
        let (u_gpu, u_mem) = match util {
            Ok(u) => (Reading::ok(u.gpu as f64), Reading::ok(u.memory as f64)),
            Err(NvmlError::NotSupported) => (Reading::unsupported("Not supported"), Reading::unsupported("Not supported")),
            Err(e) => (Reading::unavailable(&e.to_string()), Reading::unavailable(&e.to_string())),
        };
        let mem = d.memory_info();
        let (vu, vt) = match &mem {
            Ok(m) => (Reading::ok(m.used as f64), Reading::ok(m.total as f64)),
            Err(e) => (Reading::unavailable(&e.to_string()), Reading::unavailable(&e.to_string())),
        };
        let mut throttle = Vec::new();
        let mut throttle_note = String::from("From NVML clock-throttle reasons (current).");
        match d.current_throttle_reasons() {
            Ok(t) => {
                use nvml_wrapper::bitmasks::device::ThrottleReasons as T;
                if t.contains(T::HW_THERMAL_SLOWDOWN) {
                    throttle.push("Hardware thermal slowdown".to_string());
                }
                if t.contains(T::SW_THERMAL_SLOWDOWN) {
                    throttle.push("Software thermal slowdown".to_string());
                }
                if t.contains(T::HW_POWER_BRAKE_SLOWDOWN) {
                    throttle.push("Hardware power brake".to_string());
                }
                if t.contains(T::HW_SLOWDOWN) {
                    throttle.push("Hardware slowdown".to_string());
                }
                if t.contains(T::SW_POWER_CAP) {
                    throttle.push("Power cap".to_string());
                }
            }
            Err(e) => throttle_note = format!("Throttle reasons unavailable: {}", e),
        }
        Some(GpuSnap {
            name: self.name.clone(),
            driver: self.driver.clone(),
            util: u_gpu,
            mem_ctrl_util: u_mem,
            vram_used: vu,
            vram_total: vt,
            temp: rd(d.temperature(TemperatureSensor::Gpu), |v| v as f64),
            temp_slowdown: rd(d.temperature_threshold(TemperatureThreshold::Slowdown), |v| v as f64),
            hotspot: Reading::unsupported("NVML does not expose hotspot / junction temperature on GeForce GPUs"),
            fan_pct: rd(d.fan_speed(0), |v| v as f64),
            fan_rpm: rd(d.fan_speed_rpm(0), |v| v as f64),
            power_w: rd(d.power_usage(), |v| v as f64 / 1000.0),
            power_limit_w: rd(d.enforced_power_limit(), |v| v as f64 / 1000.0),
            clock_gpu: rd(d.clock_info(Clock::Graphics), |v| v as f64),
            clock_mem: rd(d.clock_info(Clock::Memory), |v| v as f64),
            clock_gpu_max: rd(d.max_clock_info(Clock::Graphics), |v| v as f64),
            clock_mem_max: rd(d.max_clock_info(Clock::Memory), |v| v as f64),
            pstate: d.performance_state().ok().and_then(|p| pstate_name(&format!("{:?}", p))),
            throttle,
            throttle_note,
        })
    }
}

fn pstate_name(dbg: &str) -> Option<String> {
    const N: [&str; 16] = ["Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten", "Eleven", "Twelve", "Thirteen", "Fourteen", "Fifteen"];
    N.iter().position(|n| *n == dbg).map(|i| format!("P{}", i))
}
