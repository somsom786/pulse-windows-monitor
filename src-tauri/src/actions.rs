//! User-initiated OS actions: Explorer integration, properties, recycle-bin moves, process termination, icons.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

pub fn reveal_in_explorer(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err("Path no longer exists".into());
    }
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn open_path(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.is_dir() {
        return Err("Only folders can be opened directly; use 'Show in Explorer' for files".into());
    }
    std::process::Command::new("explorer.exe").arg(path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn open_uri(uri: &str) -> Result<(), String> {
    // fixed allowlist; never interpolated into a shell command line
    let target = match uri {
        "ms-settings:storagesense" | "ms-settings:appsfeatures" => uri,
        "cleanmgr" => "cleanmgr.exe",
        "taskmgr" => "taskmgr.exe",
        _ => return Err("Unsupported target".into()),
    };
    let w = wide(target);
    let r = unsafe { ShellExecuteW(None, PCWSTR::null(), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOW) };
    if r.0 as isize > 32 { Ok(()) } else { Err("Windows could not open that target".into()) }
}

pub fn show_properties(path: &str) -> Result<(), String> {
    let w = wide(path);
    let verb = wide("properties");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(w.as_ptr()),
        nShow: SW_SHOW.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info).map_err(|e| e.message()) }
}

/// Locations that must never be offered for removal through PULSE.
pub fn is_protected(path: &str) -> Option<&'static str> {
    let raw = path.replace('/', "\\");
    let b = raw.as_bytes();
    // must be an absolute drive path (C:\...), with no relative components
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' || raw.split('\\').any(|c| c == ".." || c == ".") || raw.contains('\0') {
        return Some("not a plain absolute path");
    }
    // resolve junctions/symlinks/short names when the target exists
    let resolved = std::fs::canonicalize(&raw).ok().map(|p| p.to_string_lossy().trim_start_matches("\\\\?\\").to_string());
    for candidate in std::iter::once(raw.clone()).chain(resolved) {
        if let Some(why) = is_protected_norm(&candidate) {
            return Some(why);
        }
    }
    None
}

fn is_protected_norm(path: &str) -> Option<&'static str> {
    let p = path.trim_end_matches('\\').to_ascii_lowercase();
    if p.len() <= 2 || !p.is_char_boundary(2) {
        return Some("a drive root");
    }
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_ascii_lowercase();
    let drive = &p[..2];
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| format!("{}\\program files", drive)).to_ascii_lowercase();
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| format!("{}\\program files (x86)", drive)).to_ascii_lowercase();
    let pd = std::env::var("ProgramData").unwrap_or_else(|_| format!("{}\\programdata", drive)).to_ascii_lowercase();
    let prof = std::env::var("USERPROFILE").unwrap_or_default().to_ascii_lowercase();
    let exact = [format!("{}\\users", drive), prof.clone(), format!("{}\\appdata", prof), format!("{}\\appdata\\local", prof), format!("{}\\appdata\\roaming", prof), format!("{}\\documents", prof), format!("{}\\desktop", prof), format!("{}\\downloads", prof), format!("{}\\pictures", prof), format!("{}\\videos", prof), format!("{}\\music", prof), pd.clone()];
    if exact.iter().any(|e| !e.is_empty() && *e == p) {
        return Some("a core user or system folder");
    }
    for (root, why) in [(win.as_str(), "Windows system files"), (pf.as_str(), "an installed program (uninstall it from Settings instead)"), (pf86.as_str(), "an installed program (uninstall it from Settings instead)")] {
        if p == root || p.starts_with(&format!("{}\\", root)) {
            return Some(why);
        }
    }
    for frag in ["\\system volume information", "\\$recycle.bin", "\\$windows.~bt", "\\$windows.~ws", "\\recovery", "\\boot", "\\programdata\\microsoft", "\\windows.old\\windows"] {
        if p.contains(frag) {
            return Some("protected system data");
        }
    }
    let name = p.rsplit('\\').next().unwrap_or("");
    if matches!(name, "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "bootmgr" | "ntuser.dat") {
        return Some("a system file");
    }
    None
}

/// Move paths to the Recycle Bin (reversible). Windows warns if an item is too large and would be deleted permanently.
pub fn recycle(paths: &[String]) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("Nothing selected".into());
    }
    for p in paths {
        if let Some(why) = is_protected(p) {
            return Err(format!("Refusing to remove {}: it is {}.", p, why));
        }
        if !std::path::Path::new(p).exists() {
            return Err(format!("{} no longer exists.", p));
        }
    }
    // double-NUL-terminated list
    let mut buf: Vec<u16> = Vec::new();
    for p in paths {
        buf.extend(std::ffi::OsStr::new(p).encode_wide());
        buf.push(0);
    }
    buf.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(buf.as_ptr()),
        pTo: PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_WANTNUKEWARNING.0) as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(format!("Windows reported error code {:#x}", r));
    }
    if op.fAnyOperationsAborted.as_bool() {
        return Err("Cancelled".into());
    }
    Ok(paths.len())
}

const CRITICAL: &[&str] = &[
    "system", "system idle process", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "lsaiso.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "memory compression", "secure system", "sihost.exe", "ctfmon.exe", "audiodg.exe", "msmpeng.exe", "securityhealthservice.exe", "spoolsv.exe", "wudfhost.exe", "dllhost.exe", "runtimebroker.exe", "searchindexer.exe", "taskhostw.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe", "pulse.exe", "nvdisplay.container.exe",
];

pub fn can_terminate(name: &str, pid: u32) -> Result<(), String> {
    if pid <= 4 {
        return Err("This is a core system process.".into());
    }
    if CRITICAL.contains(&name.to_ascii_lowercase().as_str()) {
        return Err(format!("{} is a critical Windows or PULSE process and cannot be terminated from here.", name));
    }
    if pid == std::process::id() {
        return Err("PULSE cannot terminate itself from the process list.".into());
    }
    Ok(())
}

/// Windows shell icon for an executable, as a base64 PNG.
pub fn icon_png_base64(path: &str) -> Option<String> {
    thread_local! {
        static COM: () = unsafe {
            // the shell icon handlers want an initialized apartment on the calling thread
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        };
    }
    COM.with(|_| {});
    unsafe {
        let w = wide(path);
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(PCWSTR(w.as_ptr()), windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut sfi), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_SMALLICON);
        if r == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        let mut ii = ICONINFO::default();
        let ok = GetIconInfo(sfi.hIcon, &mut ii).is_ok();
        let mut out = None;
        if ok {
            let mut bm = BITMAP::default();
            if GetObjectW(ii.hbmColor.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) != 0 {
                let (wd, ht) = (bm.bmWidth, bm.bmHeight);
                let mut bi = BITMAPINFO::default();
                bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bi.bmiHeader.biWidth = wd;
                bi.bmiHeader.biHeight = -ht;
                bi.bmiHeader.biPlanes = 1;
                bi.bmiHeader.biBitCount = 32;
                bi.bmiHeader.biCompression = 0;
                let mut px = vec![0u8; (wd * ht * 4) as usize];
                let hdc = GetDC(None);
                let lines = GetDIBits(hdc, ii.hbmColor, 0, ht as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
                if lines != 0 {
                    // BGRA -> RGBA; if no alpha channel present use the mask
                    let has_alpha = px.chunks(4).any(|c| c[3] != 0);
                    let mut mask = vec![0u8; (wd * ht * 4) as usize];
                    let mut mask_ok = false;
                    if !has_alpha && !ii.hbmMask.is_invalid() {
                        let mut bm2 = bi;
                        mask_ok = GetDIBits(hdc, ii.hbmMask, 0, ht as u32, Some(mask.as_mut_ptr() as *mut _), &mut bm2, DIB_RGB_COLORS) != 0;
                    }
                    for (i, c) in px.chunks_mut(4).enumerate() {
                        c.swap(0, 2);
                        if !has_alpha {
                            c[3] = if mask_ok && mask[i * 4] != 0 { 0 } else { 255 };
                        }
                    }
                    let mut png_buf = Vec::new();
                    let encoded = (|| -> Result<(), png::EncodingError> {
                        let mut enc = png::Encoder::new(&mut png_buf, wd as u32, ht as u32);
                        enc.set_color(png::ColorType::Rgba);
                        enc.set_depth(png::BitDepth::Eight);
                        let mut wr = enc.write_header()?;
                        wr.write_image_data(&px)?;
                        wr.finish()
                    })();
                    if encoded.is_ok() {
                        out = Some(b64(&png_buf));
                    }
                }
                ReleaseDC(None, hdc);
            }
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(ii.hbmColor.into());
            }
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(ii.hbmMask.into());
            }
        }
        let _ = DestroyIcon(sfi.hIcon);
        out
    }
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len() * 4 / 3 + 4);
    for ch in data.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if ch.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if ch.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

/// Where a user-installed LibreHardwareMonitor lives (PULSE never downloads it).
pub fn lhm_path() -> Option<std::path::PathBuf> {
    let p = crate::settings::data_dir().join("tools").join("LibreHardwareMonitor").join("LibreHardwareMonitor.exe");
    if p.exists() { Some(p) } else { None }
}

/// Start an executable with a UAC prompt.
pub fn launch_elevated(exe: &std::path::Path) -> Result<(), String> {
    let w = wide(&exe.to_string_lossy());
    let dir = wide(&exe.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default());
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: 0,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(w.as_ptr()),
        lpDirectory: PCWSTR(dir.as_ptr()),
        nShow: SW_SHOW.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info).map_err(|e| e.message()) }
}

/// Relaunch this executable with a UAC prompt. The caller exits the process on success.
pub fn relaunch_elevated() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: 0,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(w.as_ptr()),
        nShow: SW_SHOW.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info).map_err(|e| e.message()) }
}
