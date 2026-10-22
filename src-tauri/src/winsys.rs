//! Direct Windows system queries: process table, per-CPU times, memory, page files, drives.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("PULSE reads x64 NT structures; build for a 64-bit Windows target");

use std::ffi::c_void;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::System::Threading::*;

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, buf: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC0000004u32 as i32;

#[derive(Clone, Debug, Default)]
pub struct RawProc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub create_time: u64, // FILETIME (100ns since 1601)
    pub cpu_time: u64,    // kernel + user, 100ns units
    pub threads: u32,
    pub handles: u32,
    pub session: u32,
    pub working_set: u64,
    pub private_ws: u64,
    pub peak_ws: u64,
    pub commit: u64, // PagefileUsage = private committed bytes
    pub peak_commit: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub other_bytes: u64,
}

unsafe fn rd<T: Copy>(base: *const u8, off: usize) -> T {
    std::ptr::read_unaligned(base.add(off) as *const T)
}

/// Snapshot of every process via a single NtQuerySystemInformation(SystemProcessInformation) call.
/// Includes WorkingSetPrivateSize, so private working set is exact and needs no per-process handle.
pub fn query_processes(buf: &mut Vec<u8>) -> Vec<RawProc> {
    if buf.len() < 1 << 20 {
        buf.resize(1 << 20, 0);
    }
    let mut ret: u32 = 0;
    loop {
        let st = unsafe { NtQuerySystemInformation(5, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut ret) };
        if st == STATUS_INFO_LENGTH_MISMATCH {
            let n = (ret as usize + (256 << 10)).max(buf.len() * 2);
            buf.resize(n, 0);
            continue;
        }
        if st < 0 {
            return Vec::new();
        }
        break;
    }
    let total = (ret as usize).min(buf.len());
    let mut out = Vec::with_capacity(400);
    let mut off = 0usize;
    let base = buf.as_ptr();
    let lo = base as usize;
    let hi = lo + total;
    const SPI_SIZE: usize = 0x100; // fixed part of SYSTEM_PROCESS_INFORMATION on x64
    loop {
        if off + SPI_SIZE > total {
            break;
        }
        unsafe {
            let p = base.add(off);
            let next: u32 = rd(p, 0);
            let threads: u32 = rd(p, 4);
            let wsp: i64 = rd(p, 8);
            let create: i64 = rd(p, 32);
            let user: i64 = rd(p, 40);
            let kernel: i64 = rd(p, 48);
            let name_len: u16 = rd(p, 56);
            let name_ptr: usize = rd(p, 64);
            let pid: usize = rd(p, 80);
            let ppid: usize = rd(p, 88);
            let handles: u32 = rd(p, 96);
            let session: u32 = rd(p, 100);
            let peak_ws: usize = rd(p, 136);
            let ws: usize = rd(p, 144);
            let pagefile: usize = rd(p, 184);
            let peak_pagefile: usize = rd(p, 192);
            let rd_b: i64 = rd(p, 232);
            let wr_b: i64 = rd(p, 240);
            let ot_b: i64 = rd(p, 248);
            let name_ok = name_ptr != 0 && name_len > 0 && name_len % 2 == 0 && name_ptr % 2 == 0 && name_ptr >= lo && name_ptr + name_len as usize <= hi;
            let name = if name_ok {
                let s = std::slice::from_raw_parts(name_ptr as *const u16, name_len as usize / 2);
                String::from_utf16_lossy(s)
            } else if pid == 0 {
                "System Idle Process".to_string()
            } else {
                String::new()
            };
            out.push(RawProc {
                pid: pid as u32,
                ppid: ppid as u32,
                name,
                create_time: create.max(0) as u64,
                cpu_time: (user + kernel).max(0) as u64,
                threads,
                handles,
                session,
                working_set: ws as u64,
                private_ws: wsp.max(0) as u64,
                peak_ws: peak_ws as u64,
                commit: pagefile as u64,
                peak_commit: peak_pagefile as u64,
                read_bytes: rd_b.max(0) as u64,
                write_bytes: wr_b.max(0) as u64,
                other_bytes: ot_b.max(0) as u64,
            });
            if next == 0 || next % 8 != 0 || (next as usize) < SPI_SIZE {
                break;
            }
            off += next as usize;
        }
    }
    out
}

#[derive(Clone, Copy, Default, Debug)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64, // includes idle
    pub user: u64,
}

/// Per-logical-processor cumulative times (SystemProcessorPerformanceInformation).
pub fn query_cpu_times(n_hint: usize) -> Vec<CpuTimes> {
    let mut buf = vec![0u8; 48 * n_hint.max(1).max(64)];
    let mut ret = 0u32;
    let st = unsafe { NtQuerySystemInformation(8, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut ret) };
    if st < 0 {
        return Vec::new();
    }
    let n = ret as usize / 48;
    (0..n)
        .map(|i| unsafe {
            let p = buf.as_ptr().add(i * 48);
            CpuTimes { idle: rd::<i64>(p, 0).max(0) as u64, kernel: rd::<i64>(p, 8).max(0) as u64, user: rd::<i64>(p, 16).max(0) as u64 }
        })
        .collect()
}

#[derive(Clone, Debug, Default)]
pub struct PageFile {
    pub name: String,
    pub total: u64,
    pub in_use: u64,
    pub peak: u64,
}

pub fn query_pagefiles(page_size: u64) -> Vec<PageFile> {
    let mut buf = vec![0u8; 4096];
    let mut ret = 0u32;
    let st = unsafe { NtQuerySystemInformation(18, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut ret) };
    if st < 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    let blen = (ret as usize).min(buf.len());
    let (lo, hi) = (buf.as_ptr() as usize, buf.as_ptr() as usize + blen);
    loop {
        if off + 32 > blen {
            break;
        }
        unsafe {
            let p = buf.as_ptr().add(off);
            let next: u32 = rd(p, 0);
            let total: u32 = rd(p, 4);
            let in_use: u32 = rd(p, 8);
            let peak: u32 = rd(p, 12);
            let nl: u16 = rd(p, 16);
            let np: usize = rd(p, 24);
            let name = if np >= lo && np + nl as usize <= hi && nl % 2 == 0 && np % 2 == 0 { String::from_utf16_lossy(std::slice::from_raw_parts(np as *const u16, nl as usize / 2)) } else { String::new() };
            if total > 0 {
                out.push(PageFile { name, total: total as u64 * page_size, in_use: in_use as u64 * page_size, peak: peak as u64 * page_size });
            }
            if next == 0 || next % 4 != 0 {
                break;
            }
            off += next as usize;
        }
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub commit_peak: u64,
    pub system_cache: u64,
    pub kernel_paged: u64,
    pub kernel_nonpaged: u64,
    pub page_size: u64,
    pub handles: u32,
    pub processes: u32,
    pub threads: u32,
}

pub fn query_memory() -> MemInfo {
    let mut m = MemInfo::default();
    unsafe {
        let mut pi = PERFORMANCE_INFORMATION::default();
        pi.cb = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
        if GetPerformanceInfo(&mut pi, pi.cb).is_ok() {
            let ps = pi.PageSize as u64;
            m.page_size = ps;
            m.total = pi.PhysicalTotal as u64 * ps;
            m.available = pi.PhysicalAvailable as u64 * ps;
            m.commit_total = pi.CommitTotal as u64 * ps;
            m.commit_limit = pi.CommitLimit as u64 * ps;
            m.commit_peak = pi.CommitPeak as u64 * ps;
            m.system_cache = pi.SystemCache as u64 * ps;
            m.kernel_paged = pi.KernelPaged as u64 * ps;
            m.kernel_nonpaged = pi.KernelNonpaged as u64 * ps;
            m.handles = pi.HandleCount;
            m.processes = pi.ProcessCount;
            m.threads = pi.ThreadCount;
        }
        let mut ms = MEMORYSTATUSEX::default();
        ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut ms).is_ok() {
            if m.total == 0 {
                m.total = ms.ullTotalPhys;
                m.available = ms.ullAvailPhys;
            }
            if m.page_size == 0 {
                m.page_size = 4096;
            }
        }
    }
    m
}

pub fn uptime_secs() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() / 1000 }
}

/// Full image path of a process, if it can be opened with limited rights.
pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Terminate a process only if it is still the process the user saw: the handle is opened once and its creation
/// time compared to the expected FILETIME before the same handle is used to terminate (closes the PID-reuse window).
pub fn terminate_checked(pid: u32, expected_create: u64) -> Result<(), String> {
    use windows::Win32::Foundation::FILETIME;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE, false, pid).map_err(|e| format!("Cannot open process: {}", e.message()))?;
        let (mut c, mut x, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let got = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u);
        let result = match got {
            Ok(()) => {
                // refuse OS-critical processes (terminating one bugchecks the machine); fail closed if unverifiable
                let mut critical = windows::core::BOOL(1);
                let verified = IsProcessCritical(h, &mut critical).is_ok();
                if !verified || critical.as_bool() {
                    let _ = CloseHandle(h);
                    return Err("This is a critical Windows process and cannot be ended.".to_string());
                }
                let created = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
                if created != expected_create {
                    Err("That PID now belongs to a different process; refresh and try again.".to_string())
                } else {
                    TerminateProcess(h, 1).map_err(|e| format!("Terminate failed: {}", e.message()))
                }
            }
            Err(e) => Err(format!("Cannot verify process identity: {}", e.message())),
        };
        let _ = CloseHandle(h);
        result
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub letter: String,
    pub label: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub free: u64,
}

pub fn drives() -> Vec<Drive> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", (b'A' + i as u8) as char);
        let root: Vec<u16> = format!("{}\\", letter).encode_utf16().chain(std::iter::once(0)).collect();
        let t = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
        let kind = match t {
            2 => "removable",
            3 => "fixed",
            4 => continue, // network drives are skipped: querying a disconnected share can block for seconds
            5 => "optical",
            _ => continue,
        };
        if kind == "optical" {
            continue;
        }
        let (mut free, mut total, mut tf) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut tf)).is_ok() };
        if !ok || total == 0 {
            continue;
        }
        let mut lbl = [0u16; 64];
        let mut fsn = [0u16; 32];
        unsafe {
            let _ = GetVolumeInformationW(PCWSTR(root.as_ptr()), Some(&mut lbl), None, None, None, Some(&mut fsn));
        }
        let s = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        out.push(Drive { letter, label: s(&lbl), fs: s(&fsn), kind: kind.to_string(), total, free });
    }
    out
}
