//! Hardware sensors and static hardware inventory.
//!
//! CPU/motherboard temperatures, fan speeds and power need a kernel driver on Windows. PULSE does not ship one;
//! it reads LibreHardwareMonitor / OpenHardwareMonitor through their WMI provider when that application is
//! running (the user starts it). Everything else is reported as unavailable with a reason.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wmi::{COMLibrary, Variant, WMIConnection};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HwSensor {
    pub name: String,
    pub kind: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub identifier: String,
    pub parent: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HubData {
    pub source: String,
    pub status: String,
    pub sensors: Vec<HwSensor>,
    pub updated_ms: u64,
}

pub struct SensorHub {
    pub data: Mutex<HubData>,
}

fn vs(m: &HashMap<String, Variant>, k: &str) -> Option<String> {
    match m.get(k)? {
        Variant::String(s) => Some(s.clone()),
        Variant::I4(v) => Some(v.to_string()),
        Variant::UI4(v) => Some(v.to_string()),
        Variant::UI2(v) => Some(v.to_string()),
        Variant::UI8(v) => Some(v.to_string()),
        Variant::I8(v) => Some(v.to_string()),
        Variant::UI1(v) => Some(v.to_string()),
        Variant::Bool(v) => Some(v.to_string()),
        Variant::R4(v) => Some(v.to_string()),
        Variant::R8(v) => Some(v.to_string()),
        _ => None,
    }
}

fn vf(m: &HashMap<String, Variant>, k: &str) -> Option<f64> {
    match m.get(k)? {
        Variant::R4(v) => Some(*v as f64),
        Variant::R8(v) => Some(*v),
        Variant::I4(v) => Some(*v as f64),
        Variant::UI4(v) => Some(*v as f64),
        Variant::UI2(v) => Some(*v as f64),
        Variant::UI8(v) => Some(*v as f64),
        Variant::I8(v) => Some(*v as f64),
        Variant::UI1(v) => Some(*v as f64),
        Variant::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// LibreHardwareMonitor's built-in web server (Options > Remote Web Server > Run), loopback only.
fn read_lhm_http() -> Option<(String, Vec<HwSensor>)> {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = "127.0.0.1:8085".parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    s.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(b"GET /data.json HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").ok()?;
    let mut buf = Vec::new();
    s.take(4 << 20).read_to_end(&mut buf).ok()?;
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..split]).to_ascii_lowercase();
    if !head.starts_with("http/1.1 200") && !head.starts_with("http/1.0 200") {
        return None;
    }
    let mut body = buf[split + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut i = 0;
        while i < body.len() {
            let eol = body[i..].windows(2).position(|w| w == b"\r\n")? + i;
            let n = usize::from_str_radix(std::str::from_utf8(&body[i..eol]).ok()?.split(';').next()?.trim(), 16).ok()?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(body.get(eol + 2..eol + 2 + n)?);
            i = eol + 2 + n + 2;
        }
        body = out;
    }
    let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
    let mut out = Vec::new();
    fn num(s: &str) -> Option<f64> {
        let t: String = s.chars().take_while(|c| !c.is_alphabetic() && *c != '°' && *c != '%').collect();
        let t = t.trim().replace(' ', "");
        let t = if t.contains('.') { t.replace(',', "") } else { t.replace(',', ".") };
        t.parse().ok()
    }
    fn walk(n: &serde_json::Value, path: &mut Vec<String>, out: &mut Vec<HwSensor>) {
        let text = n.get("Text").and_then(|t| t.as_str()).unwrap_or("").to_string();
        if let (Some(id), Some(val)) = (n.get("SensorId").and_then(|x| x.as_str()), n.get("Value").and_then(|x| x.as_str())) {
            let kind = ["temperature", "fan", "power", "load", "clock", "voltage", "data", "throughput", "level", "factor", "control"]
                .iter()
                .find(|k| id.contains(&format!("/{}/", k)))
                .map(|k| {
                    let mut c = k.to_string();
                    c[..1].make_ascii_uppercase();
                    c
                })
                .unwrap_or_else(|| "Other".to_string());
            if let Some(value) = num(val) {
                let parent = if path.len() >= 2 { path[path.len() - 2].clone() } else { String::new() };
                out.push(HwSensor { name: text, kind, value, min: n.get("Min").and_then(|x| x.as_str()).and_then(num), max: n.get("Max").and_then(|x| x.as_str()).and_then(num), identifier: id.to_string(), parent });
            }
            return;
        }
        if let Some(ch) = n.get("Children").and_then(|c| c.as_array()) {
            path.push(text);
            for c in ch {
                walk(c, path, out);
            }
            path.pop();
        }
    }
    walk(&v, &mut Vec::new(), &mut out);
    if out.is_empty() {
        None
    } else {
        Some(("LibreHardwareMonitor".to_string(), out))
    }
}

fn read_lhm(com: &COMLibrary) -> Option<(String, Vec<HwSensor>)> {
    if let Some(r) = read_lhm_http() {
        return Some(r);
    }
    for (ns, label) in [("ROOT\\LibreHardwareMonitor", "LibreHardwareMonitor"), ("ROOT\\OpenHardwareMonitor", "OpenHardwareMonitor")] {
        let Ok(con) = WMIConnection::with_namespace_path(ns, com.clone()) else { continue };
        let Ok(rows) = con.raw_query::<HashMap<String, Variant>>("SELECT Name, SensorType, Value, Min, Max, Identifier, Parent FROM Sensor") else { continue };
        if rows.is_empty() {
            continue;
        }
        let out = rows
            .iter()
            .filter_map(|r| {
                Some(HwSensor {
                    name: vs(r, "Name")?,
                    kind: vs(r, "SensorType")?,
                    value: vf(r, "Value")?,
                    min: vf(r, "Min"),
                    max: vf(r, "Max"),
                    identifier: vs(r, "Identifier").unwrap_or_default(),
                    parent: vs(r, "Parent").unwrap_or_default(),
                })
            })
            .collect();
        return Some((label.to_string(), out));
    }
    None
}

impl SensorHub {
    pub fn start() -> Arc<SensorHub> {
        let hub = Arc::new(SensorHub { data: Mutex::new(HubData { source: String::new(), status: "No sensor provider detected".into(), sensors: vec![], updated_ms: 0 }) });
        let h = hub.clone();
        std::thread::Builder::new()
            .name("pulse-sensors".into())
            .spawn(move || {
                let Ok(com) = COMLibrary::new() else { return };
                let mut last_ok = false;
                loop {
                    match read_lhm(&com) {
                        Some((src, sensors)) => {
                            let mut d = h.data.lock();
                            d.source = src;
                            d.status = "ok".into();
                            d.sensors = sensors;
                            d.updated_ms = crate::monitor::now_ms();
                            last_ok = true;
                        }
                        None => {
                            let mut d = h.data.lock();
                            d.source.clear();
                            d.sensors.clear();
                            d.status = "No sensor provider detected. Run LibreHardwareMonitor as administrator and enable Options > Remote Web Server > Run to expose CPU, motherboard and fan sensors.".into();
                            last_ok = false;
                        }
                    }
                    std::thread::sleep(if last_ok { Duration::from_secs(4) } else { Duration::from_secs(20) });
                }
            })
            .ok();
        hub
    }

    pub fn snapshot(&self) -> HubData {
        self.data.lock().clone()
    }

    /// Best CPU temperature candidate with its provenance.
    pub fn cpu_temp(&self) -> Option<(f64, String)> {
        let d = self.data.lock();
        if d.status != "ok" {
            return None;
        }
        let cpu_temps: Vec<&HwSensor> = d.sensors.iter().filter(|s| s.kind == "Temperature" && (s.identifier.contains("cpu") || s.parent.to_lowercase().contains("cpu"))).collect();
        for pref in ["Core (Tctl/Tdie)", "CPU Package", "Package", "Tctl/Tdie", "Core Max", "CPU (Tctl/Tdie)"] {
            if let Some(s) = cpu_temps.iter().find(|s| s.name == pref) {
                return Some((s.value, format!("{}: {}", d.source, s.name)));
            }
        }
        cpu_temps.iter().max_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal)).map(|s| (s.value, format!("{}: {}", d.source, s.name)))
    }

    pub fn cpu_power(&self) -> Option<(f64, String)> {
        let d = self.data.lock();
        if d.status != "ok" {
            return None;
        }
        d.sensors.iter().find(|s| s.kind == "Power" && (s.identifier.contains("cpu")) && (s.name.contains("Package"))).map(|s| (s.value, format!("{}: {}", d.source, s.name)))
    }
}

// ---------------- static inventory ----------------

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhysDisk {
    pub model: String,
    pub media: String,
    pub bus: String,
    pub size: u64,
    pub health: String,
    pub firmware: String,
    pub serial: String,
    pub letters: Vec<String>,
    pub temperature: Option<f64>,
    pub wear: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub read_errors: Option<f64>,
    pub write_errors: Option<f64>,
    pub reliability_status: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetAdapter {
    pub name: String,
    pub description: String,
    pub status: String,
    pub link_speed_bps: Option<f64>,
    pub mac: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub gateway: Vec<String>,
    pub dns: Vec<String>,
    pub dhcp: Option<bool>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub os: String,
    pub os_build: String,
    pub machine: String,
    pub board: String,
    pub bios: String,
    pub cpu: String,
    pub ram_desc: String,
    pub ram_modules: Vec<String>,
    pub disks: Vec<PhysDisk>,
    pub adapters: Vec<NetAdapter>,
}

fn reg_str(sub: &str, name: &str) -> String {
    use winreg::enums::*;
    use winreg::RegKey;
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub).and_then(|k| k.get_value::<String, _>(name)).unwrap_or_default()
}

fn arr_strings(m: &HashMap<String, Variant>, k: &str) -> Vec<String> {
    match m.get(k) {
        Some(Variant::Array(a)) => a
            .iter()
            .filter_map(|v| match v {
                Variant::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn media_name(m: &str, bus: &str) -> String {
    let base = match m {
        "3" => "HDD",
        "4" => "SSD",
        "5" => "Storage class memory",
        _ => "Unspecified",
    };
    let b = match bus {
        "17" => "NVMe",
        "11" => "SATA",
        "7" => "USB",
        "8" => "RAID",
        "3" => "ATA",
        "10" => "SAS",
        _ => "",
    };
    if b.is_empty() { base.to_string() } else { format!("{} ({})", base, b) }
}

pub fn inventory() -> Inventory {
    let mut inv = Inventory::default();
    let build: u32 = reg_str("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentBuild").parse().unwrap_or(0);
    let mut product = reg_str("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "ProductName");
    if build >= 22000 {
        product = product.replace("Windows 10", "Windows 11");
    }
    let disp = reg_str("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "DisplayVersion");
    inv.os = product;
    inv.os_build = format!("{} (build {})", disp, build);
    let bios = "HARDWARE\\DESCRIPTION\\System\\BIOS";
    inv.machine = format!("{} {}", reg_str(bios, "SystemManufacturer"), reg_str(bios, "SystemProductName")).trim().to_string();
    inv.board = format!("{} {}", reg_str(bios, "BaseBoardManufacturer"), reg_str(bios, "BaseBoardProduct")).trim().to_string();
    inv.bios = format!("{} {}", reg_str(bios, "BIOSVendor"), reg_str(bios, "BIOSVersion")).trim().to_string();
    let Ok(com) = COMLibrary::new() else { return inv };

    if let Ok(con) = WMIConnection::new(com.clone()) {
        if let Ok(rows) = con.raw_query::<HashMap<String, Variant>>("SELECT Capacity, Speed, ConfiguredClockSpeed, Manufacturer, PartNumber, SMBIOSMemoryType FROM Win32_PhysicalMemory") {
            let mut total = 0.0;
            let mut speed = 0.0;
            let mut ty = 0.0;
            for r in &rows {
                let cap = vf(r, "Capacity").unwrap_or(0.0);
                total += cap;
                let sp = vf(r, "ConfiguredClockSpeed").or_else(|| vf(r, "Speed")).unwrap_or(0.0);
                speed = sp;
                ty = vf(r, "SMBIOSMemoryType").unwrap_or(0.0);
                inv.ram_modules.push(format!("{} GB {} {}", (cap / 1073741824.0).round(), vs(r, "Manufacturer").unwrap_or_default().trim(), vs(r, "PartNumber").unwrap_or_default().trim()).trim().to_string());
            }
            let tname = match ty as u32 {
                26 => "DDR4",
                34 => "DDR5",
                24 => "DDR3",
                _ => "",
            };
            if total > 0.0 {
                inv.ram_desc = format!("{} GB {} {} MT/s ({} modules)", (total / 1073741824.0).round(), tname, speed, rows.len()).replace("  ", " ");
            }
        }
        // logical drive letters per physical drive, via the association classes
        let mut letters: HashMap<String, Vec<String>> = HashMap::new();
        let quoted = |v: &str| -> Option<String> {
            let i = v.find("DeviceID=\"")? + 10;
            let rest = &v[i..];
            let j = rest.rfind('"')?;
            Some(rest[..j].replace("\\\\", "\\"))
        };
        let mut part_to_disk: HashMap<String, String> = HashMap::new();
        if let Ok(rows) = con.raw_query::<HashMap<String, Variant>>("SELECT Antecedent, Dependent FROM Win32_DiskDriveToDiskPartition") {
            for r in rows {
                let (Some(a), Some(d)) = (vs(&r, "Antecedent"), vs(&r, "Dependent")) else { continue };
                let (Some(drive), Some(part)) = (quoted(&a), quoted(&d)) else { continue };
                if let Some(n) = drive.to_ascii_uppercase().split("PHYSICALDRIVE").nth(1) {
                    part_to_disk.insert(part, n.trim().to_string());
                }
            }
        }
        if let Ok(rows) = con.raw_query::<HashMap<String, Variant>>("SELECT Antecedent, Dependent FROM Win32_LogicalDiskToPartition") {
            for r in rows {
                let (Some(a), Some(d)) = (vs(&r, "Antecedent"), vs(&r, "Dependent")) else { continue };
                let (Some(part), Some(letter)) = (quoted(&a), quoted(&d)) else { continue };
                if let Some(idx) = part_to_disk.get(&part) {
                    letters.entry(idx.clone()).or_default().push(letter);
                }
            }
        }
        if let Ok(st) = WMIConnection::with_namespace_path("ROOT\\Microsoft\\Windows\\Storage", com.clone()) {
            if let Ok(pds) = st.raw_query::<HashMap<String, Variant>>("SELECT DeviceId, FriendlyName, MediaType, BusType, Size, HealthStatus, FirmwareVersion, SerialNumber FROM MSFT_PhysicalDisk") {
                let rel = st.raw_query::<HashMap<String, Variant>>("SELECT DeviceId, Temperature, Wear, PowerOnHours, ReadErrorsTotal, WriteErrorsTotal FROM MSFT_StorageReliabilityCounter");
                for d in pds {
                    let id = vs(&d, "DeviceId").unwrap_or_default();
                    let health = match vs(&d, "HealthStatus").as_deref() {
                        Some("0") => "Healthy",
                        Some("1") => "Warning",
                        Some("2") => "Unhealthy",
                        _ => "Unknown",
                    };
                    let mut pd = PhysDisk {
                        model: vs(&d, "FriendlyName").unwrap_or_default().trim().to_string(),
                        media: media_name(&vs(&d, "MediaType").unwrap_or_default(), &vs(&d, "BusType").unwrap_or_default()),
                        bus: vs(&d, "BusType").unwrap_or_default(),
                        size: vf(&d, "Size").unwrap_or(0.0) as u64,
                        health: health.to_string(),
                        firmware: vs(&d, "FirmwareVersion").unwrap_or_default(),
                        serial: vs(&d, "SerialNumber").unwrap_or_default().trim().to_string(),
                        letters: letters.get(&id).cloned().unwrap_or_default(),
                        reliability_status: "Unavailable".into(),
                        ..Default::default()
                    };
                    match &rel {
                        Ok(rows) => {
                            if let Some(r) = rows.iter().find(|r| vs(r, "DeviceId").as_deref() == Some(id.as_str())) {
                                pd.temperature = vf(r, "Temperature").filter(|t| *t > 0.0 && *t < 150.0);
                                pd.wear = vf(r, "Wear");
                                pd.power_on_hours = vf(r, "PowerOnHours");
                                pd.read_errors = vf(r, "ReadErrorsTotal");
                                pd.write_errors = vf(r, "WriteErrorsTotal");
                                pd.reliability_status = if pd.temperature.is_some() || pd.power_on_hours.is_some() || pd.wear.is_some() { "ok".into() } else { "Drive did not report SMART counters to Windows (some require administrator rights)".into() };
                            }
                        }
                        Err(_) => pd.reliability_status = "SMART counters require administrator rights".into(),
                    }
                    inv.disks.push(pd);
                }
            }
        }
        // network adapters (physical) + IP configuration
        let adapters = con.raw_query::<HashMap<String, Variant>>("SELECT Name, NetConnectionID, NetConnectionStatus, MACAddress, Speed, InterfaceIndex FROM Win32_NetworkAdapter WHERE PhysicalAdapter=TRUE");
        let cfg = con.raw_query::<HashMap<String, Variant>>("SELECT InterfaceIndex, IPAddress, DefaultIPGateway, DNSServerSearchOrder, DHCPEnabled FROM Win32_NetworkAdapterConfiguration WHERE IPEnabled=TRUE");
        if let Ok(adapters) = adapters {
            for a in adapters {
                let idx = vs(&a, "InterfaceIndex").unwrap_or_default();
                let mut na = NetAdapter {
                    name: vs(&a, "NetConnectionID").unwrap_or_else(|| vs(&a, "Name").unwrap_or_default()),
                    description: vs(&a, "Name").unwrap_or_default(),
                    status: match vs(&a, "NetConnectionStatus").as_deref() {
                        Some("2") => "Connected",
                        Some("0") | Some("7") | Some("1") => "Disconnected",
                        Some(_) => "Other",
                        None => "Not enabled",
                    }
                    .into(),
                    link_speed_bps: vf(&a, "Speed").filter(|v| *v > 0.0 && *v < 1e13),
                    mac: vs(&a, "MACAddress").unwrap_or_default(),
                    ..Default::default()
                };
                if let Ok(cfg) = &cfg {
                    if let Some(c) = cfg.iter().find(|c| vs(c, "InterfaceIndex").as_deref() == Some(idx.as_str())) {
                        for ip in arr_strings(c, "IPAddress") {
                            if ip.contains(':') { na.ipv6.push(ip) } else { na.ipv4.push(ip) }
                        }
                        na.gateway = arr_strings(c, "DefaultIPGateway");
                        na.dns = arr_strings(c, "DNSServerSearchOrder");
                        na.dhcp = match c.get("DHCPEnabled") {
                            Some(Variant::Bool(b)) => Some(*b),
                            _ => None,
                        };
                    }
                }
                if na.status == "Not enabled" && na.ipv4.is_empty() {
                    continue;
                }
                inv.adapters.push(na);
            }
        }
    }
    inv
}

pub struct InvCache {
    pub at: Instant,
    pub data: Option<Inventory>,
}
