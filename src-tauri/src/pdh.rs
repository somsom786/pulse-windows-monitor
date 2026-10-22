//! Thin wrapper over the Windows Performance Data Helper (PDH) API.
//! One query holds all counters so a single `collect()` refreshes everything.

use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::System::Performance::*;

pub struct Pdh {
    query: PDH_HQUERY,
    counters: Vec<PDH_HCOUNTER>,
    scratch: Vec<u8>,
}

fn wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

const PDH_MORE_DATA: u32 = 0x800007D2;

impl Pdh {
    pub fn new() -> Option<Pdh> {
        let mut q = PDH_HQUERY::default();
        let r = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut q) };
        if r != 0 {
            return None;
        }
        Some(Pdh { query: q, counters: Vec::new(), scratch: vec![0u8; 64 * 1024] })
    }

    /// Returns the counter index, or None if the counter doesn't exist on this machine.
    pub fn add(&mut self, path: &str) -> Option<usize> {
        let w = wide(path);
        let mut h = PDH_HCOUNTER::default();
        let r = unsafe { PdhAddEnglishCounterW(self.query, PCWSTR(w.as_ptr()), 0, &mut h) };
        if r != 0 {
            return None;
        }
        self.counters.push(h);
        Some(self.counters.len() - 1)
    }

    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    pub fn value(&self, idx: usize) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let r = unsafe { PdhGetFormattedCounterValue(self.counters[idx], PDH_FMT_DOUBLE, None, &mut v) };
        if r != 0 || (v.CStatus != 0 && v.CStatus != 1) {
            return None;
        }
        Some(unsafe { v.Anonymous.doubleValue })
    }

    /// Values of every instance of a wildcard counter, as (instance name, value).
    pub fn array(&mut self, idx: usize) -> Vec<(String, f64)> {
        let h = self.counters[idx];
        let mut size: u32 = self.scratch.len() as u32;
        let mut count: u32 = 0;
        let mut r = unsafe { PdhGetFormattedCounterArrayW(h, PDH_FMT_DOUBLE, &mut size, &mut count, Some(self.scratch.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) };
        if r == PDH_MORE_DATA {
            self.scratch.resize(size as usize + 1024, 0);
            size = self.scratch.len() as u32;
            r = unsafe { PdhGetFormattedCounterArrayW(h, PDH_FMT_DOUBLE, &mut size, &mut count, Some(self.scratch.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) };
        }
        if r != 0 {
            return Vec::new();
        }
        let items = self.scratch.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let it = unsafe { &*items.add(i) };
            if it.FmtValue.CStatus != 0 && it.FmtValue.CStatus != 1 {
                continue;
            }
            let name = unsafe { it.szName.to_string().unwrap_or_default() };
            out.push((name, unsafe { it.FmtValue.Anonymous.doubleValue }));
        }
        out
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
