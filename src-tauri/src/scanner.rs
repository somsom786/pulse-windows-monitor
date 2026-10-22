//! Parallel NTFS directory scanner.
//!
//! Directories are enumerated with `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`, which returns
//! logical size (EndOfFile), allocated size (AllocationSize), attributes and the file ID for many entries
//! per syscall. Worker threads enumerate directories; a single builder thread appends nodes to a compact
//! arena, so no locking is needed on the tree itself.
//!
//! Reparse-point directories (junctions, symlinks, mount points) are recorded but never followed, which
//! prevents cycles. Files that share a file ID (hard links) are counted once.

use crate::categories::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WIN32_ERROR};
use windows::Win32::Storage::FileSystem::*;

pub const F_DIR: u8 = 1;
pub const F_LINK: u8 = 2;
pub const F_HARDDUP: u8 = 4;
pub const F_ERR: u8 = 8;
pub const F_HIDDEN: u8 = 16;
pub const F_SYSTEM: u8 = 32;
pub const F_CLOUD: u8 = 64;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Node {
    pub parent: u32,
    pub child_start: u32,
    pub child_count: u32,
    pub name_off: u32,
    pub name_len: u16,
    pub flags: u8,
    pub cat: u8,
    /// Logical size (sum of file lengths for directories).
    pub size: u64,
    /// Allocated size on disk (cluster-rounded; 0 for resident/dehydrated/compressed-away data).
    pub alloc: u64,
    pub files: u32,
    pub mtime: u32,
    pub file_id: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanMeta {
    pub id: String,
    pub root: String,
    pub started_unix: u64,
    pub duration_ms: u64,
    pub files: u64,
    pub dirs: u64,
    pub total_logical: u64,
    pub total_alloc: u64,
    pub errors: u64,
    pub volume_total: u64,
    pub volume_free: u64,
    pub hardlink_saved: u64,
    pub hardlink_files: u64,
    pub cancelled: bool,
    pub fs_name: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub path: String,
    pub code: u32,
}

#[derive(Serialize, Deserialize)]
pub struct ScanTree {
    pub meta: ScanMeta,
    pub nodes: Vec<Node>,
    pub names: Vec<u8>,
    pub errors: Vec<ScanError>,
}

impl ScanTree {
    pub fn name(&self, id: u32) -> &str {
        let n = &self.nodes[id as usize];
        let s = &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize];
        std::str::from_utf8(s).unwrap_or("?")
    }

    pub fn is_dir(&self, id: u32) -> bool {
        self.nodes[id as usize].flags & F_DIR != 0
    }

    pub fn children(&self, id: u32) -> std::ops::Range<u32> {
        let n = &self.nodes[id as usize];
        n.child_start..n.child_start + n.child_count
    }

    pub fn path(&self, id: u32) -> String {
        let mut parts: Vec<&str> = Vec::new();
        let mut cur = id;
        loop {
            parts.push(self.name(cur));
            let p = self.nodes[cur as usize].parent;
            if cur == 0 || p == u32::MAX {
                break;
            }
            cur = p;
        }
        parts.reverse();
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i == 0 {
                out.push_str(p);
            } else {
                if !out.ends_with('\\') {
                    out.push('\\');
                }
                out.push_str(p);
            }
        }
        out
    }

    /// Resolve an absolute path (case-insensitive) to a node id if it lies inside this scan.
    pub fn find_path(&self, path: &str) -> Option<u32> {
        let root = self.name(0).trim_end_matches('\\').to_ascii_lowercase();
        let norm = path.replace('/', "\\");
        let norm_trim = norm.trim_end_matches('\\');
        let lower = norm_trim.to_ascii_lowercase();
        if lower == root {
            return Some(0);
        }
        if !lower.starts_with(&format!("{}\\", root)) {
            return None;
        }
        let rest = &norm_trim[root.len() + 1..];
        let mut cur = 0u32;
        for comp in rest.split('\\') {
            if comp.is_empty() {
                continue;
            }
            let mut found = None;
            for c in self.children(cur) {
                if self.name(c).eq_ignore_ascii_case(comp) {
                    found = Some(c);
                    break;
                }
            }
            cur = found?;
        }
        Some(cur)
    }
}

#[derive(Clone, Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub root: String,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub current: String,
    pub elapsed_ms: u64,
    pub queued: u64,
}

struct RawEntry {
    name: String,
    flags: u8,
    size: u64,
    alloc: u64,
    mtime: u32,
    file_id: u64,
}

struct DirResult {
    node: u32,
    path: String,
    entries: Vec<RawEntry>,
    error: Option<u32>,
}

fn to_wide(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = OsStr::new(s).encode_wide().collect();
    v.push(0);
    v
}

/// Convert to an extended-length path usable by the Win32 API.
fn extended(path: &str) -> String {
    if path.starts_with(r"\\?\") {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{}", rest)
    } else {
        format!(r"\\?\{}", path)
    }
}

fn filetime_to_unix(ft: i64) -> u32 {
    if ft <= 116_444_736_000_000_000 {
        return 0;
    }
    ((ft - 116_444_736_000_000_000) / 10_000_000).clamp(0, u32::MAX as i64) as u32
}

const ATTR_DIR: u32 = 0x10;
const ATTR_REPARSE: u32 = 0x400;
const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_OPEN: u32 = 0x40000;
const ATTR_RECALL_DATA: u32 = 0x400000;

fn is_cloud_tag(tag: u32) -> bool {
    (tag & 0xFFFF0FFF) == 0x9000001A
}

/// Reparse tags that are links to other data (never counted, never followed).
fn is_link_tag(tag: u32) -> bool {
    matches!(tag, 0xA000000C | 0xA0000003 | 0x8000001B | 0x80000023 | 0xA000001D | 0x80000025)
}

const HDR: usize = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);

/// Enumerate one directory. Returns the entries read plus an error code if enumeration stopped abnormally
/// (entries read before the error are still returned so partial results are visible).
fn enumerate_dir(path: &str, buf: &mut [u64]) -> (Vec<RawEntry>, Option<u32>) {
    let wide = to_wide(&extended(path));
    let h: HANDLE = match unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0x0001, // FILE_LIST_DIRECTORY
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    } {
        Ok(h) => h,
        Err(e) => return (Vec::new(), Some((e.code().0 as u32) & 0xFFFF)),
    };

    let cap = buf.len() * 8;
    let base = buf.as_mut_ptr() as *mut u8;
    let mut out: Vec<RawEntry> = Vec::new();
    let mut class = FileIdExtdDirectoryRestartInfo;
    let mut err: Option<u32> = None;
    'outer: loop {
        let r = unsafe { GetFileInformationByHandleEx(h, class, base as *mut _, cap as u32) };
        class = FileIdExtdDirectoryInfo;
        if let Err(e) = r {
            let code = (e.code().0 as u32) & 0xFFFF;
            if code != 18 {
                // 18 = ERROR_NO_MORE_FILES is the normal end of enumeration
                err = Some(code);
            }
            break;
        }
        let mut off = 0usize;
        loop {
            if off + HDR > cap || off % 8 != 0 {
                err = Some(13); // ERROR_INVALID_DATA
                break 'outer;
            }
            let p = unsafe { base.add(off) as *const FILE_ID_EXTD_DIR_INFO };
            let e = unsafe { &*p };
            let name_bytes = e.FileNameLength as usize;
            if name_bytes % 2 != 0 || off + HDR + name_bytes > cap {
                err = Some(13);
                break 'outer;
            }
            let name_len = name_bytes / 2;
            let name_slice = unsafe { std::slice::from_raw_parts(base.add(off + HDR) as *const u16, name_len) };
            let skip = (name_len == 1 && name_slice[0] == b'.' as u16) || (name_len == 2 && name_slice[0] == b'.' as u16 && name_slice[1] == b'.' as u16);
            if !skip {
                let attrs = e.FileAttributes;
                let tag = e.ReparsePointTag;
                let mut flags = 0u8;
                let is_dir = attrs & ATTR_DIR != 0;
                if is_dir {
                    flags |= F_DIR;
                }
                if attrs & ATTR_HIDDEN != 0 {
                    flags |= F_HIDDEN;
                }
                if attrs & ATTR_SYSTEM != 0 {
                    flags |= F_SYSTEM;
                }
                let mut size = e.EndOfFile.max(0) as u64;
                let mut alloc = e.AllocationSize.max(0) as u64;
                if attrs & ATTR_REPARSE != 0 {
                    if is_cloud_tag(tag) {
                        flags |= F_CLOUD; // placeholder: real allocation as reported (0 when dehydrated); cloud dirs are followed
                    } else if is_dir {
                        // junction / symlink / mount point / unknown reparse directory: never followed
                        flags |= F_LINK;
                    } else if is_link_tag(tag) {
                        flags |= F_LINK;
                        size = 0;
                        alloc = 0;
                    }
                    // other file reparse points (WOF compression, dedup, ...) are data-bearing: keep reported sizes
                }
                if attrs & (ATTR_OFFLINE | ATTR_RECALL_OPEN | ATTR_RECALL_DATA) != 0 {
                    flags |= F_CLOUD;
                }
                if is_dir {
                    size = 0;
                    alloc = 0;
                }
                let id_bytes: [u8; 16] = unsafe { std::ptr::read_unaligned(&e.FileId as *const _ as *const [u8; 16]) };
                let lo = u64::from_le_bytes(id_bytes[..8].try_into().unwrap());
                let hi = u64::from_le_bytes(id_bytes[8..].try_into().unwrap());
                out.push(RawEntry {
                    name: String::from_utf16_lossy(name_slice),
                    flags,
                    size,
                    alloc,
                    mtime: filetime_to_unix(e.LastWriteTime),
                    // only NTFS-style IDs (upper half zero) are used for hard-link detection
                    file_id: if hi == 0 { lo } else { 0 },
                });
            }
            let next = e.NextEntryOffset as usize;
            if next == 0 {
                break;
            }
            if next % 8 != 0 || next < HDR {
                err = Some(13);
                break 'outer;
            }
            off += next;
        }
    }
    unsafe {
        let _ = CloseHandle(h);
    }
    (out, err)
}

/// Slower path for file systems that don't support extended directory info.
fn enumerate_fallback(path: &str) -> (Vec<RawEntry>, Option<u32>) {
    let rd = match std::fs::read_dir(extended(path)) {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Some(e.raw_os_error().unwrap_or(5) as u32)),
    };
    let mut out = Vec::new();
    for ent in rd.flatten() {
        let Ok(md) = ent.metadata() else { continue };
        let ft = md.file_type();
        let mut flags = 0u8;
        let mut size = md.len();
        if ft.is_dir() {
            flags |= F_DIR;
            size = 0;
        }
        if ft.is_symlink() {
            flags |= F_LINK;
            size = 0;
        }
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        out.push(RawEntry {
            name: ent.file_name().to_string_lossy().into_owned(),
            flags,
            size,
            alloc: (size + 4095) & !4095,
            mtime,
            file_id: 0,
        });
    }
    (out, None)
}

fn worker(work: Receiver<(u32, String)>, results: Sender<DirResult>, cancel: Arc<AtomicBool>) {
    let mut buf = vec![0u64; 32 * 1024]; // 256 KiB, 8-byte aligned
    while let Ok((node, path)) = work.recv() {
        if cancel.load(Ordering::Relaxed) {
            let _ = results.send(DirResult { node, path, entries: vec![], error: None });
            continue;
        }
        let (mut entries, mut error) = enumerate_dir(&path, &mut buf);
        if entries.is_empty() && matches!(error, Some(87) | Some(50) | Some(1)) {
            let (e2, err2) = enumerate_fallback(&path);
            entries = e2;
            error = err2;
        }
        if results.send(DirResult { node, path, entries, error }).is_err() {
            break;
        }
    }
}

pub fn volume_space(root: &str) -> (u64, u64, String) {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW};
    let mut r = root.to_string();
    if !r.ends_with('\\') {
        // use the drive root for volume queries
        if r.len() >= 2 && r.as_bytes()[1] == b':' {
            r = format!("{}\\", &r[..2]);
        }
    } else if r.len() > 3 {
        r = format!("{}\\", &r[..2]);
    }
    let w = to_wide(&r);
    let mut avail = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    unsafe {
        let _ = GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut avail), Some(&mut total), Some(&mut free));
    }
    let mut fsn = [0u16; 64];
    unsafe {
        let _ = GetVolumeInformationW(PCWSTR(w.as_ptr()), None, None, None, None, Some(&mut fsn));
    }
    let n = fsn.iter().position(|&c| c == 0).unwrap_or(0);
    (total, free, String::from_utf16_lossy(&fsn[..n]))
}

pub fn new_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{:x}{:x}", t.as_secs(), t.subsec_nanos() & 0xFFFFF)
}

/// Run a scan. `progress` is invoked about every 150ms from the calling thread.
pub fn scan(root: &str, cancel: Arc<AtomicBool>, mut progress: impl FnMut(&ScanProgress)) -> ScanTree {
    let started = Instant::now();
    let started_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let root_norm = if root.len() == 2 && root.as_bytes()[1] == b':' { format!("{}\\", root) } else { root.to_string() };

    let mut nodes: Vec<Node> = Vec::with_capacity(1 << 20);
    let mut names: Vec<u8> = Vec::with_capacity(32 << 20);
    let mut errors: Vec<ScanError> = Vec::new();
    let mut error_count = 0u64;

    // root node
    names.extend_from_slice(root_norm.as_bytes());
    nodes.push(Node {
        parent: u32::MAX,
        child_start: 0,
        child_count: 0,
        name_off: 0,
        name_len: root_norm.len().min(u16::MAX as usize) as u16,
        flags: F_DIR,
        cat: CAT_FOLDER,
        size: 0,
        alloc: 0,
        files: 0,
        mtime: 0,
        file_id: 0,
    });

    let (work_tx, work_rx) = unbounded::<(u32, String)>();
    let (res_tx, res_rx) = unbounded::<DirResult>();
    let n_workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let mut handles = Vec::new();
    for _ in 0..n_workers {
        let (w, r, c) = (work_rx.clone(), res_tx.clone(), cancel.clone());
        handles.push(std::thread::spawn(move || worker(w, r, c)));
    }
    drop(res_tx);
    drop(work_rx);

    let pending = Arc::new(AtomicU64::new(1));
    let _ = work_tx.send((0, root_norm.clone()));

    let mut files = 0u64;
    let mut dirs = 1u64;
    let mut bytes = 0u64;
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut current = root_norm.clone();

    while pending.load(Ordering::Relaxed) > 0 {
        let r = match res_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => r,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if last_emit.elapsed() > Duration::from_millis(150) {
                    last_emit = Instant::now();
                    progress(&ScanProgress { root: root_norm.clone(), files, dirs, bytes, errors: error_count, current: current.clone(), elapsed_ms: started.elapsed().as_millis() as u64, queued: pending.load(Ordering::Relaxed) });
                }
                continue;
            }
            Err(_) => break,
        };
        pending.fetch_sub(1, Ordering::Relaxed);
        if let Some(code) = r.error {
            error_count += 1;
            nodes[r.node as usize].flags |= F_ERR;
            if errors.len() < 5000 {
                errors.push(ScanError { path: r.path.clone(), code });
            }
        }
        let start = nodes.len() as u32;
        nodes[r.node as usize].child_start = start;
        nodes[r.node as usize].child_count = r.entries.len() as u32;
        for e in r.entries {
            let id = nodes.len() as u32;
            let name_off = names.len() as u32;
            let nb = e.name.as_bytes();
            let name_len = nb.len().min(u16::MAX as usize);
            names.extend_from_slice(&nb[..name_len]);
            let is_dir = e.flags & F_DIR != 0;
            let cat = if is_dir { CAT_FOLDER } else { category_for_name(&e.name) };
            if is_dir {
                dirs += 1;
                if e.flags & F_LINK == 0 && !cancel.load(Ordering::Relaxed) {
                    pending.fetch_add(1, Ordering::Relaxed);
                    let mut child_path = r.path.clone();
                    if !child_path.ends_with('\\') {
                        child_path.push('\\');
                    }
                    child_path.push_str(&e.name);
                    let _ = work_tx.send((id, child_path));
                }
            } else {
                files += 1;
                bytes += e.alloc;
            }
            nodes.push(Node { parent: r.node, child_start: 0, child_count: 0, name_off, name_len: name_len as u16, flags: e.flags, cat, size: e.size, alloc: e.alloc, files: 0, mtime: e.mtime, file_id: e.file_id });
        }
        current = r.path;
        if last_emit.elapsed() > Duration::from_millis(150) {
            last_emit = Instant::now();
            progress(&ScanProgress { root: root_norm.clone(), files, dirs, bytes, errors: error_count, current: current.clone(), elapsed_ms: started.elapsed().as_millis() as u64, queued: pending.load(Ordering::Relaxed) });
        }
    }
    drop(work_tx);
    for h in handles {
        let _ = h.join();
    }

    let cancelled = cancel.load(Ordering::Relaxed);
    let (hl_saved, hl_files) = finalize(&mut nodes);

    let mut total_logical = 0u64;
    let mut total_alloc = 0u64;
    for c in children_of(&nodes, 0) {
        total_logical += nodes[c].size;
        total_alloc += nodes[c].alloc;
    }
    let (vt, vf, fsn) = volume_space(&root_norm);
    let meta = ScanMeta {
        id: new_id(),
        root: root_norm,
        started_unix,
        duration_ms: started.elapsed().as_millis() as u64,
        files,
        dirs,
        total_logical,
        total_alloc,
        errors: error_count,
        volume_total: vt,
        volume_free: vf,
        hardlink_saved: hl_saved,
        hardlink_files: hl_files,
        cancelled,
        fs_name: fsn,
    };
    names.shrink_to_fit();
    nodes.shrink_to_fit();
    progress(&ScanProgress { root: meta.root.clone(), files, dirs, bytes, errors: error_count, current: String::new(), elapsed_ms: meta.duration_ms, queued: 0 });
    ScanTree { meta, nodes, names, errors }
}

fn children_of(nodes: &[Node], id: usize) -> impl Iterator<Item = usize> {
    let n = &nodes[id];
    (n.child_start as usize)..(n.child_start as usize + n.child_count as usize)
}

/// Detect hard links, then roll file sizes up into directories. Returns (bytes saved, hard-linked file count).
fn finalize(nodes: &mut [Node]) -> (u64, u64) {
    // hard-link dedupe: later nodes sharing a file id with an earlier node count as zero
    let mut ids: Vec<(u64, u32)> = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.flags & (F_DIR | F_LINK) == 0 && n.file_id != 0 && (n.size > 0 || n.alloc > 0) {
            ids.push((n.file_id, i as u32));
        }
    }
    ids.sort_unstable();
    let mut saved = 0u64;
    let mut hl = 0u64;
    let mut i = 0;
    while i < ids.len() {
        let mut j = i + 1;
        while j < ids.len() && ids[j].0 == ids[i].0 {
            let idx = ids[j].1 as usize;
            saved += nodes[idx].alloc;
            hl += 1;
            nodes[idx].flags |= F_HARDDUP;
            nodes[idx].alloc = 0;
            nodes[idx].size = 0;
            j += 1;
        }
        i = j;
    }
    // roll up: parents always have a smaller id than their children
    for i in (1..nodes.len()).rev() {
        let n = nodes[i];
        let p = n.parent as usize;
        if n.flags & F_DIR != 0 {
            nodes[p].size += n.size;
            nodes[p].alloc += n.alloc;
            nodes[p].files += n.files;
        } else {
            nodes[p].size += n.size;
            nodes[p].alloc += n.alloc;
            nodes[p].files += 1;
        }
    }
    (saved, hl)
}

// ---------- persistence ----------

pub fn scans_dir() -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join("PULSE").join("scans");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn save_tree(tree: &ScanTree) -> std::io::Result<()> {
    let dir = scans_dir();
    let path = dir.join(format!("{}.scan", tree.meta.id));
    let tmp = dir.join(format!("{}.tmp", tree.meta.id));
    {
        let f = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&tmp)?);
        let mut enc = lz4_flex::frame::FrameEncoder::new(f);
        bincode::serialize_into(&mut enc, tree).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let mut f = enc.finish().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        use std::io::Write;
        f.flush()?;
    }
    std::fs::rename(&tmp, &path)?;
    let mut idx = list_meta();
    idx.retain(|m| m.id != tree.meta.id);
    idx.push(tree.meta.clone());
    write_index(&idx)?;
    Ok(())
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn load_tree(id: &str) -> Result<ScanTree, String> {
    if !valid_id(id) {
        return Err("Invalid scan id".into());
    }
    let path = scans_dir().join(format!("{}.scan", id));
    let f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let dec = lz4_flex::frame::FrameDecoder::new(std::io::BufReader::with_capacity(1 << 20, f));
    bincode::deserialize_from(dec).map_err(|e| e.to_string())
}

pub fn list_meta() -> Vec<ScanMeta> {
    let p = scans_dir().join("index.json");
    let mut v: Vec<ScanMeta> = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    v.retain(|m| scans_dir().join(format!("{}.scan", m.id)).exists());
    v.sort_by(|a, b| b.started_unix.cmp(&a.started_unix));
    v
}

fn write_index(v: &[ScanMeta]) -> std::io::Result<()> {
    std::fs::write(scans_dir().join("index.json"), serde_json::to_string(v).unwrap_or_default())
}

pub fn delete_scan(id: &str) {
    if !valid_id(id) {
        return;
    }
    let _ = std::fs::remove_file(scans_dir().join(format!("{}.scan", id)));
    let mut idx = list_meta();
    idx.retain(|m| m.id != id);
    let _ = write_index(&idx);
}

#[allow(dead_code)]
pub fn last_error() -> WIN32_ERROR {
    unsafe { GetLastError() }
}
