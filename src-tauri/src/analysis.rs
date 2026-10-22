//! Read-only queries over a completed scan: treemap data, tables, filters, cleanup insights.

use crate::categories::*;
use crate::scanner::*;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: u32,
    pub parent: i64,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub alloc: u64,
    pub files: u32,
    pub cat: u8,
    pub flags: u8,
    pub mtime: u32,
    pub ext: String,
}

pub fn row(t: &ScanTree, id: u32, with_path: bool) -> Row {
    let n = &t.nodes[id as usize];
    let name = t.name(id).to_string();
    let is_dir = n.flags & F_DIR != 0;
    Row {
        id,
        parent: if n.parent == u32::MAX { -1 } else { n.parent as i64 },
        ext: if is_dir { String::new() } else { ext_of(&name).to_ascii_lowercase() },
        path: if with_path { t.path(id) } else { String::new() },
        name,
        is_dir,
        size: n.size,
        alloc: n.alloc,
        files: n.files,
        cat: n.cat,
        flags: n.flags,
        mtime: n.mtime,
    }
}

fn val(n: &Node, metric: &str) -> u64 {
    if metric == "logical" { n.size } else { n.alloc }
}

// ---------- tree navigation ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crumb {
    pub id: u32,
    pub name: String,
}

pub fn breadcrumbs(t: &ScanTree, id: u32) -> Vec<Crumb> {
    let mut v = Vec::new();
    let mut cur = id;
    loop {
        v.push(Crumb { id: cur, name: t.name(cur).to_string() });
        let p = t.nodes[cur as usize].parent;
        if p == u32::MAX {
            break;
        }
        cur = p;
    }
    v.reverse();
    v
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Children {
    pub node: Row,
    pub rows: Vec<Row>,
    pub total: usize,
}

pub fn children(t: &ScanTree, id: u32, metric: &str, limit: usize) -> Children {
    let mut ids: Vec<u32> = t.children(id).collect();
    ids.sort_by_key(|&c| Reverse(val(&t.nodes[c as usize], metric)));
    let total = ids.len();
    ids.truncate(limit);
    Children { node: row(t, id, true), rows: ids.iter().map(|&c| row(t, c, true)).collect(), total }
}

// ---------- treemap ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tm {
    pub id: i64,
    pub name: String,
    pub value: u64,
    pub alloc: u64,
    pub size: u64,
    pub cat: u8,
    pub is_dir: bool,
    pub other: u32,
    pub files: u32,
    pub flags: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Tm>>,
}

fn tm_node(t: &ScanTree, id: u32, metric: &str, level: u32, forced: u32, per_level: usize, root_val: u64) -> Tm {
    let n = &t.nodes[id as usize];
    let is_dir = n.flags & F_DIR != 0;
    let mut node = Tm { id: id as i64, name: t.name(id).to_string(), value: val(n, metric), alloc: n.alloc, size: n.size, cat: n.cat, is_dir, other: 0, files: n.files, flags: n.flags, children: None };
    // Always expand the first `forced` levels; below that expand only folders big enough to be visible on screen.
    let frac = node.value as f64 / root_val.max(1) as f64;
    let expand = is_dir && n.child_count > 0 && (level < forced || (frac >= 0.004 && level < 9));
    if expand {
        let mut ids: Vec<u32> = t.children(id).filter(|&c| val(&t.nodes[c as usize], metric) > 0).collect();
        ids.sort_by_key(|&c| Reverse(val(&t.nodes[c as usize], metric)));
        let limit = per_level.max(6);
        let mut kids: Vec<Tm> = Vec::new();
        let (mut ov, mut oa, mut os, mut oc) = (0u64, 0u64, 0u64, 0u32);
        let parent_val = node.value.max(1);
        for (i, &c) in ids.iter().enumerate() {
            let cv = val(&t.nodes[c as usize], metric);
            // fold items that would be sub-pixel into "other"
            if i < limit && (cv as f64 / parent_val as f64) > 0.0008 {
                kids.push(tm_node(t, c, metric, level + 1, forced, (per_level * 3 / 4).max(10), root_val));
            } else {
                ov += cv;
                oa += t.nodes[c as usize].alloc;
                os += t.nodes[c as usize].size;
                oc += 1;
            }
        }
        if oc > 0 {
            kids.push(Tm { id: -1, name: format!("{} smaller items", oc), value: ov, alloc: oa, size: os, cat: CAT_OTHER, is_dir: false, other: oc, files: oc, flags: 0, children: None });
        }
        node.children = Some(kids);
    }
    node
}

pub fn treemap(t: &ScanTree, id: u32, metric: &str, depth: u32) -> Tm {
    let rv = val(&t.nodes[id as usize], metric);
    tm_node(t, id, metric, 0, depth.clamp(1, 4), 70, rv)
}

// ---------- search / filter ----------

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub name: Option<String>,
    pub exts: Option<Vec<String>>,
    pub cats: Option<Vec<u8>>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub folder: Option<u32>,
    pub kind: Option<String>, // "files" | "dirs" | "all"
    pub older_than_days: Option<u32>,
    pub metric: Option<String>,
    pub sort: Option<String>, // size | name | mtime | files
    pub desc: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub rows: Vec<Row>,
    pub matched: usize,
    pub matched_bytes: u64,
}

fn now_unix() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

pub fn search(t: &ScanTree, f: &Filter) -> SearchResult {
    let metric = f.metric.as_deref().unwrap_or("alloc");
    let needle = f.name.as_ref().map(|s| s.to_lowercase()).filter(|s| !s.is_empty());
    let exts: Option<Vec<String>> = f.exts.as_ref().map(|v| v.iter().map(|e| e.trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect()).filter(|v: &Vec<String>| !v.is_empty());
    let kind = f.kind.as_deref().unwrap_or("all");
    let min = f.min_size.unwrap_or(0);
    let max = f.max_size.unwrap_or(u64::MAX);
    let cutoff = f.older_than_days.map(|d| now_unix().saturating_sub(d * 86400));
    // restrict to descendants of a folder by walking its subtree
    let mut cands: Vec<u32> = Vec::new();
    let mut matched_bytes = 0u64;
    let mut visit = |id: u32| {
        let n = &t.nodes[id as usize];
        let is_dir = n.flags & F_DIR != 0;
        if kind == "files" && is_dir {
            return;
        }
        if kind == "dirs" && !is_dir {
            return;
        }
        let v = val(n, metric);
        if v < min || v > max {
            return;
        }
        if let Some(c) = cutoff {
            if n.mtime == 0 || n.mtime > c {
                return;
            }
        }
        if let Some(cs) = &f.cats {
            if !cs.is_empty() && (is_dir || !cs.contains(&n.cat)) {
                return;
            }
        }
        if exts.is_some() || needle.is_some() {
            let name = t.name(id);
            if let Some(ex) = &exts {
                if is_dir || !ex.iter().any(|e| ext_of(name).eq_ignore_ascii_case(e)) {
                    return;
                }
            }
            if let Some(nd) = &needle {
                if !name.to_lowercase().contains(nd.as_str()) {
                    return;
                }
            }
        }
        matched_bytes += if is_dir { 0 } else { v };
        cands.push(id);
    };
    match f.folder {
        Some(root) if root != 0 => {
            let mut stack = vec![root];
            while let Some(d) = stack.pop() {
                for c in t.children(d) {
                    visit(c);
                    if t.nodes[c as usize].flags & F_DIR != 0 {
                        stack.push(c);
                    }
                }
            }
        }
        _ => {
            for id in 1..t.nodes.len() as u32 {
                visit(id);
            }
        }
    }
    let matched = cands.len();
    let desc = f.desc.unwrap_or(true);
    match f.sort.as_deref().unwrap_or("size") {
        "name" => cands.sort_by_cached_key(|&i| t.name(i).to_lowercase()),
        "mtime" => cands.sort_by_key(|&i| t.nodes[i as usize].mtime),
        "files" => cands.sort_by_key(|&i| t.nodes[i as usize].files),
        _ => cands.sort_by_key(|&i| val(&t.nodes[i as usize], metric)),
    }
    if desc {
        cands.reverse();
    }
    let off = f.offset.unwrap_or(0);
    let lim = f.limit.unwrap_or(300).min(2000);
    let rows = cands.iter().skip(off).take(lim).map(|&i| row(t, i, true)).collect();
    SearchResult { rows, matched, matched_bytes }
}

// ---------- overview analysis ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExtStat {
    pub ext: String,
    pub count: u64,
    pub alloc: u64,
    pub size: u64,
    pub cat: u8,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CatStat {
    pub cat: u8,
    pub name: String,
    pub count: u64,
    pub alloc: u64,
    pub size: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub meta: ScanMeta,
    pub top_files: Vec<Row>,
    pub top_folders: Vec<Row>,
    pub exts: Vec<ExtStat>,
    pub cats: Vec<CatStat>,
    pub unaccounted: Unaccounted,
    pub error_samples: Vec<ScanError>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Unaccounted {
    pub volume_used: u64,
    pub scanned_alloc: u64,
    pub gap: i64,
    pub covers_whole_volume: bool,
    pub notes: Vec<String>,
}

pub fn overview(t: &ScanTree) -> Overview {
    let n = t.nodes.len();
    // top files
    let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::new();
    let mut ext_map: HashMap<String, (u64, u64, u64, u8)> = HashMap::new();
    let mut cat_map = [(0u64, 0u64, 0u64); 12];
    for id in 1..n as u32 {
        let nd = &t.nodes[id as usize];
        if nd.flags & F_DIR != 0 {
            continue;
        }
        if nd.alloc > 0 || nd.size > 0 {
            heap.push(Reverse((nd.alloc.max(nd.size / 64), id)));
            if heap.len() > 100 {
                heap.pop();
            }
        }
        let name = t.name(id);
        let ext = ext_of(name);
        let key = ext.to_ascii_lowercase();
        let e = ext_map.entry(key).or_insert((0, 0, 0, nd.cat));
        e.0 += 1;
        e.1 += nd.alloc;
        e.2 += nd.size;
        let c = (nd.cat as usize).min(11);
        cat_map[c].0 += 1;
        cat_map[c].1 += nd.alloc;
        cat_map[c].2 += nd.size;
    }
    let mut top: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(x)| x).collect();
    top.sort_by_key(|x| Reverse(x.0));
    let top_files: Vec<Row> = top.iter().map(|&(_, id)| row(t, id, true)).collect();

    // largest "leaf-like" folders: heavy directories where no single child dir dominates
    let mut folders: Vec<(u64, u32)> = Vec::new();
    for id in 1..n as u32 {
        let nd = &t.nodes[id as usize];
        if nd.flags & F_DIR == 0 || nd.alloc < 50 * 1024 * 1024 {
            continue;
        }
        let mut max_child = 0u64;
        for c in t.children(id) {
            let cn = &t.nodes[c as usize];
            if cn.flags & F_DIR != 0 {
                max_child = max_child.max(cn.alloc);
            }
        }
        if (max_child as f64) < nd.alloc as f64 * 0.8 {
            folders.push((nd.alloc, id));
        }
    }
    folders.sort_by_key(|x| Reverse(x.0));
    folders.truncate(80);
    let top_folders = folders.iter().map(|&(_, id)| row(t, id, true)).collect();

    let mut exts: Vec<ExtStat> = ext_map.into_iter().map(|(k, v)| ExtStat { ext: if k.is_empty() { "(none)".into() } else { k }, count: v.0, alloc: v.1, size: v.2, cat: v.3 }).collect();
    exts.sort_by_key(|e| Reverse(e.alloc));
    exts.truncate(60);
    let cats = cat_map.iter().enumerate().map(|(i, c)| CatStat { cat: i as u8, name: CATEGORY_NAMES[i].to_string(), count: c.0, alloc: c.1, size: c.2 }).collect();

    let m = &t.meta;
    let root_is_drive = m.root.trim_end_matches('\\').len() == 2;
    let used = m.volume_total.saturating_sub(m.volume_free);
    let gap = used as i64 - m.total_alloc as i64;
    let mut notes = Vec::new();
    if root_is_drive {
        notes.push("Space not visible to a normal-user scan includes NTFS metadata ($MFT, $LogFile, $Bitmap), the System Volume Information folder (restore points / shadow copies), reserved storage for Windows Update, and any folder PULSE was denied access to.".to_string());
        if m.errors > 0 {
            notes.push(format!("{} folders could not be opened (access denied or in use). Their contents are not included. Running PULSE as administrator may reveal more.", m.errors));
        }
        if gap > 0 && gap as u64 > used / 50 {
            notes.push("The unexplained gap is larger than 2% of used space — likely restore points / shadow copies, reserved storage or protected system data.".to_string());
        }
        if gap < 0 {
            notes.push("Scanned allocation exceeds volume-used space. This can happen with compressed/sparse or deduplicated files whose reported allocation overstates real usage, or files changing during the scan.".to_string());
        }
    } else {
        notes.push("Only a folder was scanned, so no comparison with the whole volume is made.".to_string());
    }
    if m.hardlink_files > 0 {
        notes.push(format!("{} hard-linked file entries ({}) were counted once, so folder totals can be smaller than the sum of the file sizes shown in Explorer.", m.hardlink_files, crate::fmt_bytes(m.hardlink_saved)));
    }
    let unaccounted = Unaccounted { volume_used: used, scanned_alloc: m.total_alloc, gap, covers_whole_volume: root_is_drive, notes };
    Overview { meta: m.clone(), top_files, top_folders, exts, cats, unaccounted, error_samples: t.errors.iter().take(200).cloned().collect() }
}

// ---------- well-known locations ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub label: String,
    pub group: String,
    pub path: String,
    pub node: Option<u32>,
    pub alloc: u64,
    pub size: u64,
    pub files: u32,
    pub risk: String,
    pub what: String,
    pub cleanup: String,
}

fn envp(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

pub fn locations(t: &ScanTree) -> Vec<Location> {
    let drive = t.name(0).trim_end_matches('\\').to_string();
    let prof = envp("USERPROFILE");
    let local = envp("LOCALAPPDATA");
    let roaming = envp("APPDATA");
    let win = envp("SystemRoot");
    let pd = envp("ProgramData");
    // (label, group, path, risk, what, cleanup)
    let mut defs: Vec<(String, &str, String, &str, &str, &str)> = vec![
        ("Downloads".into(), "Downloads", format!("{}\\Downloads", prof), "review", "Files you downloaded. Often contains installers, archives and ISOs that are no longer needed.", "Review in Explorer and delete what you no longer need. Settings > System > Storage > Temporary files can also clear the Downloads folder (only if you tick it)."),
        ("User temp files".into(), "Temporary", format!("{}\\Temp", local), "safe", "Temporary files created by applications. Files in use are skipped by Windows.", "Settings > System > Storage > Temporary files, or run Disk Cleanup (cleanmgr)."),
        ("Windows temp".into(), "Temporary", format!("{}\\Temp", win), "safe", "System-wide temporary files.", "Settings > System > Storage > Temporary files, or Disk Cleanup > Clean up system files."),
        ("Windows Update download cache".into(), "Temporary", format!("{}\\SoftwareDistribution\\Download", win), "safe", "Downloaded Windows Update payloads. Windows re-downloads what it needs.", "Disk Cleanup > Clean up system files > Windows Update Cleanup."),
        ("Memory dumps".into(), "Temporary", format!("{}\\Minidump", win), "safe", "Crash dump files from system crashes.", "Disk Cleanup > Clean up system files > System error memory dump files."),
        ("Windows error reports".into(), "Temporary", format!("{}\\Microsoft\\Windows\\WER", pd), "safe", "Windows Error Reporting queue and archive.", "Disk Cleanup > Windows Error Reporting files."),
        ("Recycle Bin".into(), "Temporary", format!("{}\\$Recycle.Bin", drive), "safe", "Deleted files waiting to be permanently removed (all users, this drive).", "Right-click the Recycle Bin > Empty Recycle Bin."),
        ("Installer package cache".into(), "Temporary", format!("{}\\Package Cache", pd), "review", "Cached installers used to repair/uninstall some programs (Visual Studio, etc.).", "Do not delete by hand — uninstall the owning program or use its installer's cleanup option."),
        ("Windows Installer cache".into(), "System", format!("{}\\Installer", win), "system", "Needed to repair, update or uninstall installed programs. Protected: deleting breaks uninstalls.", "Not safe to clean manually."),
        ("Component store (WinSxS)".into(), "System", format!("{}\\WinSxS", win), "system", "Windows component store. Mostly hard links, so this number is not purely additional space.", "Only via DISM: 'Dism.exe /Online /Cleanup-Image /StartComponentCleanup' (administrator). Never delete files manually."),
        ("Windows folder".into(), "System", win.clone(), "system", "Windows operating system files.", "Protected. Use Storage Sense / Disk Cleanup."),
        ("Chrome cache".into(), "Caches", format!("{}\\Google\\Chrome\\User Data\\Default\\Cache", local), "safe", "Browser cache; rebuilt automatically.", "Chrome > Settings > Privacy > Delete browsing data > Cached images and files."),
        ("Edge cache".into(), "Caches", format!("{}\\Microsoft\\Edge\\User Data\\Default\\Cache", local), "safe", "Browser cache; rebuilt automatically.", "Edge > Settings > Privacy > Clear browsing data."),
        ("NVIDIA shader cache (DX)".into(), "Caches", format!("{}\\NVIDIA\\DXCache", local), "safe", "Compiled shader cache. Games rebuild it (first launch may stutter).", "Settings > System > Storage > Temporary files > DirectX Shader Cache."),
        ("D3D shader cache".into(), "Caches", format!("{}\\D3DSCache", local), "safe", "DirectX shader cache; rebuilt as needed.", "Disk Cleanup > DirectX Shader Cache."),
        ("npm cache".into(), "Caches", format!("{}\\npm-cache", local), "safe", "Node package manager download cache.", "Run 'npm cache clean --force'."),
        ("pnpm store".into(), "Caches", format!("{}\\pnpm", local), "review", "Shared pnpm package store (projects hard-link to it).", "Run 'pnpm store prune'."),
        ("Cargo registry & git cache".into(), "Caches", format!("{}\\.cargo\\registry", prof), "review", "Downloaded Rust crates and sources.", "Rebuilt on demand; 'cargo cache' (third-party) or delete the folder if you accept re-downloads."),
        ("Gradle caches".into(), "Caches", format!("{}\\.gradle", prof), "review", "Gradle build caches and wrapper distributions.", "Delete the 'caches' folder if you accept re-downloads."),
        ("NuGet packages".into(), "Caches", format!("{}\\.nuget\\packages", prof), "review", "Restored .NET packages.", "'dotnet nuget locals all --clear'."),
        ("pip cache".into(), "Caches", format!("{}\\pip\\Cache", local), "safe", "Python package download cache.", "Run 'pip cache purge'."),
        ("Hugging Face cache".into(), "Caches", format!("{}\\.cache\\huggingface", prof), "review", "Downloaded ML models and datasets.", "Use 'huggingface-cli delete-cache'."),
        ("Ollama models".into(), "Caches", format!("{}\\.ollama\\models", prof), "review", "Downloaded local LLM models.", "Remove models with 'ollama rm <model>'."),
        ("Docker Desktop data".into(), "Caches", format!("{}\\Docker", local), "review", "Docker Desktop WSL virtual disks (images, containers, volumes).", "Use 'docker system prune' inside Docker, then compact the VHDX."),
        ("AppData\\Local".into(), "AppData", local.clone(), "review", "Per-user, per-machine application data and caches.", "See breakdown below; clean through each application where possible."),
        ("AppData\\Roaming".into(), "AppData", roaming.clone(), "review", "Per-user application settings and data.", "Clean through each application."),
        ("ProgramData".into(), "AppData", pd.clone(), "review", "Machine-wide application data.", "Clean through each application."),
        ("Users profile".into(), "AppData", prof.clone(), "review", "Your whole user profile.", ""),
    ];
    // game library folders
    let pf = [format!("{}\\Program Files", drive), format!("{}\\Program Files (x86)", drive)];
    let mut game_dirs: Vec<(String, String)> = vec![
        ("Games".into(), format!("{}\\Games", drive)),
        ("Steam library".into(), format!("{}\\SteamLibrary", drive)),
        ("Steam (default)".into(), format!("{}\\Steam", pf[1])),
        ("Epic Games".into(), format!("{}\\Epic Games", pf[0])),
        ("Epic Games (root)".into(), format!("{}\\Epic Games", drive)),
        ("GOG Galaxy games".into(), format!("{}\\GOG Galaxy\\Games", pf[1])),
        ("GOG Games".into(), format!("{}\\GOG Games", drive)),
        ("Riot Games".into(), format!("{}\\Riot Games", drive)),
        ("Battle.net / Blizzard".into(), format!("{}\\Battle.net", drive)),
        ("Xbox / Game Pass".into(), format!("{}\\XboxGames", drive)),
        ("EA Games".into(), format!("{}\\EA Games", pf[0])),
        ("Ubisoft".into(), format!("{}\\Ubisoft", pf[1])),
        ("Rockstar Games".into(), format!("{}\\Rockstar Games", pf[0])),
    ];
    game_dirs.dedup();
    for (l, p) in game_dirs {
        defs.push((l, "Games", p, "review", "Installed game library. Uninstall through the game launcher to remove cleanly.", "Uninstall via Steam / Epic / launcher, or Settings > Apps > Installed apps."));
    }
    let mut out = Vec::new();
    for (label, group, path, risk, what, cleanup) in defs {
        if path.is_empty() || path.starts_with('\\') {
            continue;
        }
        let node = t.find_path(&path);
        let (alloc, size, files) = node.map(|n| (t.nodes[n as usize].alloc, t.nodes[n as usize].size, t.nodes[n as usize].files)).unwrap_or((0, 0, 0));
        if node.is_none() {
            continue;
        }
        out.push(Location { label, group: group.to_string(), path, node, alloc, size, files, risk: risk.to_string(), what: what.to_string(), cleanup: cleanup.to_string() });
    }
    out
}

/// Children of a well-known folder (e.g. each game in the Games folder, each app in AppData\Local).
pub fn top_children_of(t: &ScanTree, path: &str, n: usize) -> Vec<Row> {
    match t.find_path(path) {
        Some(id) => {
            let mut ids: Vec<u32> = t.children(id).filter(|&c| t.nodes[c as usize].flags & F_DIR != 0).collect();
            ids.sort_by_key(|&c| Reverse(t.nodes[c as usize].alloc));
            ids.truncate(n);
            ids.iter().map(|&c| row(t, c, true)).collect()
        }
        None => vec![],
    }
}

// ---------- old installers / archives ----------

pub fn old_installers(t: &ScanTree) -> Vec<Row> {
    let cutoff = now_unix().saturating_sub(60 * 86400);
    let mut cands: Vec<u32> = Vec::new();
    for id in 1..t.nodes.len() as u32 {
        let n = &t.nodes[id as usize];
        if n.flags & (F_DIR | F_LINK | F_HARDDUP) != 0 || n.alloc < 20 * 1024 * 1024 || n.mtime == 0 || n.mtime > cutoff {
            continue;
        }
        let ext = ext_of(t.name(id)).to_ascii_lowercase();
        if matches!(ext.as_str(), "msi" | "msix" | "iso" | "zip" | "7z" | "rar" | "tar" | "gz" | "cab" | "exe" | "img" | "tgz") {
            cands.push(id);
        }
    }
    cands.sort_by_key(|&i| Reverse(t.nodes[i as usize].alloc));
    let mut out = Vec::new();
    for id in cands {
        let r = row(t, id, true);
        let low = r.path.to_ascii_lowercase();
        if low.contains("\\windows\\") || low.contains("\\program files") || low.contains("\\node_modules\\") || low.contains("\\programdata\\") || low.contains("\\appdata\\local\\programs\\") || low.contains("\\steamapps\\") || low.contains("\\.rustup\\") || low.contains("\\site-packages\\") {
            continue;
        }
        if r.ext == "exe" && !(low.contains("\\downloads\\") || low.contains("\\desktop\\")) {
            continue;
        }
        out.push(r);
        if out.len() >= 150 {
            break;
        }
    }
    out
}

// ---------- installed applications ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub location: String,
    pub size: u64,
    pub source: String, // "measured" | "installer estimate" | "unknown"
}

pub fn installed_apps(t: Option<&ScanTree>) -> Vec<App> {
    use winreg::enums::*;
    use winreg::RegKey;
    let mut apps: HashMap<String, App> = HashMap::new();
    let roots: [(RegKey, &str); 3] = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_CURRENT_USER), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ];
    for (hive, path) in roots.iter() {
        let Ok(key) = hive.open_subkey_with_flags(path, KEY_READ) else { continue };
        for sub in key.enum_keys().flatten() {
            let Ok(k) = key.open_subkey_with_flags(&sub, KEY_READ) else { continue };
            let name: String = k.get_value("DisplayName").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let sys: u32 = k.get_value("SystemComponent").unwrap_or(0);
            if sys == 1 {
                continue;
            }
            let publisher: String = k.get_value("Publisher").unwrap_or_default();
            let version: String = k.get_value("DisplayVersion").unwrap_or_default();
            let loc: String = k.get_value::<String, _>("InstallLocation").unwrap_or_default().trim_matches('"').trim_end_matches('\\').to_string();
            let est_kb: u32 = k.get_value("EstimatedSize").unwrap_or(0);
            let mut size = 0u64;
            let mut source = "unknown";
            if !loc.is_empty() {
                if let Some(tree) = t {
                    if let Some(id) = tree.find_path(&loc) {
                        size = tree.nodes[id as usize].alloc;
                        source = "measured";
                    }
                }
            }
            if size == 0 && est_kb > 0 {
                size = est_kb as u64 * 1024;
                source = "installer estimate";
            }
            apps.entry(name.clone()).or_insert(App { name, publisher, version, location: loc, size, source: source.to_string() });
        }
    }
    let mut v: Vec<App> = apps.into_values().collect();
    v.sort_by_key(|a| Reverse(a.size));
    v
}

// ---------- scan comparison ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRow {
    pub path: String,
    pub before: u64,
    pub after: u64,
    pub delta: i64,
    pub depth: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub rows: Vec<DiffRow>,
    pub total_before: u64,
    pub total_after: u64,
    pub older: String,
    pub newer: String,
}

fn dir_map(t: &ScanTree, min: u64, max_depth: u32) -> HashMap<String, u64> {
    let mut m = HashMap::new();
    let mut stack = vec![(0u32, 0u32)];
    while let Some((id, d)) = stack.pop() {
        m.insert(t.path(id).to_ascii_lowercase(), t.nodes[id as usize].alloc);
        if d >= max_depth {
            continue;
        }
        for c in t.children(id) {
            let n = &t.nodes[c as usize];
            if n.flags & F_DIR != 0 && n.alloc >= min {
                stack.push((c, d + 1));
            }
        }
    }
    m
}

pub fn diff(a: &ScanTree, b: &ScanTree) -> Diff {
    let (older, newer) = if a.meta.started_unix <= b.meta.started_unix { (a, b) } else { (b, a) };
    let ma = dir_map(older, 20 << 20, 7);
    let mb = dir_map(newer, 20 << 20, 7);
    let mut rows = Vec::new();
    let mut keys: Vec<&String> = ma.keys().chain(mb.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let before = *ma.get(k).unwrap_or(&0);
        let after = *mb.get(k).unwrap_or(&0);
        let delta = after as i64 - before as i64;
        if delta.unsigned_abs() >= 50 << 20 {
            rows.push(DiffRow { path: k.clone(), before, after, delta, depth: k.matches('\\').count() as u32 });
        }
    }
    rows.sort_by_key(|r| Reverse(r.delta.unsigned_abs()));
    rows.truncate(300);
    Diff { rows, total_before: older.meta.total_alloc, total_after: newer.meta.total_alloc, older: older.meta.id.clone(), newer: newer.meta.id.clone() }
}

// ---------- duplicate detection ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    pub size: u64,
    pub wasted: u64,
    pub files: Vec<Row>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DupProgress {
    pub stage: String,
    pub done: u64,
    pub total: u64,
}

pub fn find_duplicates(
    t: &ScanTree,
    min_size: u64,
    cancel: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(DupProgress),
) -> Vec<DupGroup> {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    let mut by_size: HashMap<u64, Vec<u32>> = HashMap::new();
    for id in 1..t.nodes.len() as u32 {
        let n = &t.nodes[id as usize];
        if n.flags & (F_DIR | F_LINK | F_HARDDUP | F_CLOUD) != 0 || n.size < min_size {
            continue;
        }
        by_size.entry(n.size).or_default().push(id);
    }
    by_size.retain(|_, v| v.len() > 1);
    let mut cands: Vec<(u64, u32)> = Vec::new();
    for (s, v) in &by_size {
        for &id in v {
            cands.push((*s, id));
        }
    }
    // skip protected locations
    let skip = |p: &str| {
        let l = p.to_ascii_lowercase();
        l.contains("\\windows\\") || l.contains("\\winsxs\\") || l.contains("\\system volume information") || l.contains("\\$recycle.bin")
    };
    let total = cands.len() as u64;
    let mut done = 0u64;
    // stage 1: hash the first 64 KiB
    let mut groups: HashMap<(u64, [u8; 32]), Vec<u32>> = HashMap::new();
    let mut buf = vec![0u8; 65536];
    for (s, id) in &cands {
        if cancel.load(Ordering::Relaxed) {
            return vec![];
        }
        done += 1;
        if done % 200 == 0 {
            progress(DupProgress { stage: "Sampling file headers".into(), done, total });
        }
        let path = t.path(*id);
        if skip(&path) {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        let n = f.read(&mut buf).unwrap_or(0);
        let h = blake3::hash(&buf[..n]);
        groups.entry((*s, *h.as_bytes())).or_default().push(*id);
    }
    groups.retain(|_, v| v.len() > 1);
    // stage 2: full hash
    let total2: u64 = groups.values().map(|v| v.len() as u64).sum();
    let mut done2 = 0u64;
    let mut full: HashMap<(u64, [u8; 32]), Vec<u32>> = HashMap::new();
    for ((s, _), ids) in groups {
        for id in ids {
            if cancel.load(Ordering::Relaxed) {
                return vec![];
            }
            done2 += 1;
            if done2 % 5 == 0 {
                progress(DupProgress { stage: "Hashing candidate files".into(), done: done2, total: total2 });
            }
            let path = t.path(id);
            let Ok(mut f) = std::fs::File::open(&path) else { continue };
            let mut hasher = blake3::Hasher::new();
            let mut big = vec![0u8; 1 << 20];
            loop {
                match f.read(&mut big) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        hasher.update(&big[..n]);
                    }
                }
                if cancel.load(Ordering::Relaxed) {
                    return vec![];
                }
            }
            full.entry((s, *hasher.finalize().as_bytes())).or_default().push(id);
        }
    }
    let mut out: Vec<DupGroup> = full
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|((s, _), ids)| DupGroup { size: s, wasted: s * (ids.len() as u64 - 1), files: ids.iter().map(|&i| row(t, i, true)).collect() })
        .collect();
    out.sort_by_key(|g| Reverse(g.wasted));
    out.truncate(300);
    progress(DupProgress { stage: "Done".into(), done: total2, total: total2 });
    out
}
