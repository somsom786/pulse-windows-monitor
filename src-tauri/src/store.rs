//! Bounded local history in SQLite. Writes are batched; unknown values are stored as NULL, never 0.

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct SampleRow {
    pub ts: i64,
    pub cpu: Option<f64>,
    pub ram_used: Option<f64>,
    pub ram_avail: Option<f64>,
    pub commit: Option<f64>,
    pub gpu_util: Option<f64>,
    pub vram_used: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_power: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub disk_r: Option<f64>,
    pub disk_w: Option<f64>,
    pub net_rx: Option<f64>,
    pub net_tx: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct SnapRow {
    pub pid: u32,
    pub ct: u64,
    pub name: String,
    pub pws: u64,
    pub ws: u64,
    pub commit: u64,
    pub cpu: f64,
    pub gpu: Option<f64>,
    pub gpu_mem: Option<u64>,
}

struct Inner {
    conn: Connection,
    samples: Vec<SampleRow>,
    snaps: Vec<(i64, Vec<SnapRow>)>,
}

pub struct Store {
    inner: Mutex<Inner>,
    path: std::path::PathBuf,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub t: i64,
    pub cpu: Option<f64>,
    pub cpu_max: Option<f64>,
    pub ram_used: Option<f64>,
    pub ram_used_max: Option<f64>,
    pub ram_avail_min: Option<f64>,
    pub commit: Option<f64>,
    pub gpu_util: Option<f64>,
    pub gpu_util_max: Option<f64>,
    pub vram_used: Option<f64>,
    pub vram_used_max: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_power: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub disk_r: Option<f64>,
    pub disk_w: Option<f64>,
    pub net_rx: Option<f64>,
    pub net_tx: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SnapProc {
    pub pid: u32,
    pub name: String,
    pub private_ws: u64,
    pub working_set: u64,
    pub commit: u64,
    pub cpu: f64,
    pub gpu: Option<f64>,
    pub gpu_mem: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SnapAt {
    pub ts: i64,
    pub requested: i64,
    pub procs: Vec<SnapProc>,
    /// Same data grouped by executable name (private working set summed)
    pub apps: Vec<AppAgg>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppAgg {
    pub name: String,
    pub count: u32,
    pub private_ws: u64,
    pub cpu: f64,
    pub gpu_mem: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpikeRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub value: f64,
    pub baseline: Option<f64>,
    pub summary: String,
    pub detail: serde_json::Value,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AlertRow {
    pub id: i64,
    pub ts: i64,
    pub kind: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PeakRow {
    pub name: String,
    pub peak_private_ws: u64,
    pub peak_ts: i64,
    pub max_instances: u32,
    pub peak_cpu: f64,
    pub peak_gpu_mem: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GrowthRow {
    pub pid: u32,
    pub name: String,
    pub start_ws: u64,
    pub now_ws: u64,
    pub growth_per_hour: f64,
    pub minutes: f64,
    pub samples: u32,
    pub note: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoreInfo {
    pub path: String,
    pub size_bytes: u64,
    pub samples: i64,
    pub snapshots: i64,
    pub spikes: i64,
    pub oldest_ts: Option<i64>,
    pub newest_ts: Option<i64>,
}

impl Store {
    pub fn open() -> rusqlite::Result<Store> {
        let path = crate::settings::data_dir().join("history.db");
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-8000;
             CREATE TABLE IF NOT EXISTS samples(
               ts INTEGER PRIMARY KEY, cpu REAL, ram_used REAL, ram_avail REAL, commit_b REAL,
               gpu_util REAL, vram_used REAL, gpu_temp REAL, gpu_power REAL, cpu_temp REAL,
               disk_r REAL, disk_w REAL, net_rx REAL, net_tx REAL);
             CREATE TABLE IF NOT EXISTS proc_snaps(
               ts INTEGER NOT NULL, pid INTEGER NOT NULL, ct INTEGER NOT NULL, name TEXT NOT NULL,
               pws INTEGER, ws INTEGER, commit_b INTEGER, cpu REAL, gpu REAL, gpu_mem INTEGER);
             CREATE INDEX IF NOT EXISTS idx_snaps_ts ON proc_snaps(ts);
             CREATE TABLE IF NOT EXISTS spikes(
               id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT NOT NULL, value REAL,
               baseline REAL, summary TEXT, detail TEXT);
             CREATE INDEX IF NOT EXISTS idx_spikes_ts ON spikes(ts);
             CREATE TABLE IF NOT EXISTS alerts(
               id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, kind TEXT, message TEXT);",
        )?;
        // one-time migration: earlier builds could store the same process row twice (periodic + spike snapshot)
        let has_uq: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='idx_snaps_uq'", [], |r| r.get(0)).unwrap_or(1);
        if has_uq == 0 {
            conn.execute_batch(
                "DELETE FROM proc_snaps WHERE rowid NOT IN (SELECT MIN(rowid) FROM proc_snaps GROUP BY ts,pid,ct);
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_snaps_uq ON proc_snaps(ts,pid,ct);",
            )?;
        }
        Ok(Store { inner: Mutex::new(Inner { conn, samples: Vec::new(), snaps: Vec::new() }), path })
    }

    /// Read-only connection (WAL allows it to run concurrently with the writer). Falls back to an empty database.
    fn reader(&self) -> Connection {
        use rusqlite::OpenFlags;
        let c = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .or_else(|_| Connection::open_in_memory())
            .expect("sqlite");
        let _ = c.busy_timeout(std::time::Duration::from_millis(3000));
        c
    }

    pub fn push_sample(&self, s: SampleRow) {
        let mut g = self.inner.lock();
        g.samples.push(s);
        if g.samples.len() >= 60 {
            Self::flush_locked(&mut g);
        }
    }

    pub fn push_snap(&self, ts: i64, rows: Vec<SnapRow>) {
        let mut g = self.inner.lock();
        g.snaps.push((ts, rows));
    }

    pub fn flush(&self) {
        let mut g = self.inner.lock();
        Self::flush_locked(&mut g);
    }

    fn flush_locked(g: &mut Inner) {
        if g.samples.is_empty() && g.snaps.is_empty() {
            return;
        }
        let samples = std::mem::take(&mut g.samples);
        let snaps = std::mem::take(&mut g.snaps);
        let res: rusqlite::Result<()> = (|| {
            let tx = g.conn.transaction()?;
            {
                let mut st = tx.prepare_cached("INSERT OR REPLACE INTO samples VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)")?;
                for s in &samples {
                    st.execute(params![s.ts, s.cpu, s.ram_used, s.ram_avail, s.commit, s.gpu_util, s.vram_used, s.gpu_temp, s.gpu_power, s.cpu_temp, s.disk_r, s.disk_w, s.net_rx, s.net_tx])?;
                }
                let mut sp = tx.prepare_cached("INSERT OR IGNORE INTO proc_snaps VALUES(?,?,?,?,?,?,?,?,?,?)")?;
                for (ts, rows) in &snaps {
                    for r in rows {
                        sp.execute(params![ts, r.pid, r.ct as i64, r.name, r.pws as i64, r.ws as i64, r.commit as i64, r.cpu, r.gpu, r.gpu_mem.map(|v| v as i64)])?;
                    }
                }
            }
            tx.commit()
        })();
        if res.is_err() {
            // keep the batch for the next attempt, but stay bounded if the disk is unavailable
            if g.samples.len() + samples.len() < 20_000 {
                let mut keep = samples;
                keep.append(&mut g.samples);
                g.samples = keep;
            }
            if g.snaps.len() + snaps.len() < 2_000 {
                let mut keep = snaps;
                keep.append(&mut g.snaps);
                g.snaps = keep;
            }
        }
    }

    pub fn add_spike(&self, ts: i64, kind: &str, value: f64, baseline: Option<f64>, summary: &str, detail: &serde_json::Value) -> i64 {
        let g = self.inner.lock();
        g.conn
            .execute("INSERT INTO spikes(ts,kind,value,baseline,summary,detail) VALUES(?,?,?,?,?,?)", params![ts, kind, value, baseline, summary, detail.to_string()])
            .ok();
        g.conn.last_insert_rowid()
    }

    pub fn add_alert(&self, ts: i64, kind: &str, msg: &str) {
        let g = self.inner.lock();
        let _ = g.conn.execute("INSERT INTO alerts(ts,kind,message) VALUES(?,?,?)", params![ts, kind, msg]);
    }

    /// Most recent alert time per cooldown key, so cooldowns survive restarts.
    pub fn recent_alert_keys(&self, since: i64) -> Vec<(String, i64)> {
        let conn = self.reader();
        let Ok(mut st) = conn.prepare("SELECT kind, MAX(ts) FROM alerts WHERE ts>=? GROUP BY kind") else { return vec![] };
        st.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map(|i| i.flatten().collect()).unwrap_or_default()
    }

    pub fn alerts(&self, limit: i64) -> Vec<AlertRow> {
        let conn = self.reader();
        let Ok(mut st) = conn.prepare("SELECT id,ts,kind,message FROM alerts ORDER BY ts DESC LIMIT ?") else { return vec![] };
        st.query_map([limit], |r| Ok(AlertRow { id: r.get(0)?, ts: r.get(1)?, kind: r.get(2)?, message: r.get(3)? })).map(|i| i.flatten().collect()).unwrap_or_default()
    }

    pub fn series(&self, from: i64, to: i64, bucket_ms: i64) -> Vec<Point> {
        self.flush();
        let conn = self.reader();
        let b = bucket_ms.max(1000);
        let Ok(mut st) = conn.prepare(
            "SELECT (ts/?1)*?1 AS t, AVG(cpu), MAX(cpu), AVG(ram_used), MAX(ram_used), MIN(ram_avail), AVG(commit_b),
                    AVG(gpu_util), MAX(gpu_util), AVG(vram_used), MAX(vram_used), MAX(gpu_temp), AVG(gpu_power), MAX(cpu_temp),
                    AVG(disk_r), AVG(disk_w), AVG(net_rx), AVG(net_tx)
             FROM samples WHERE ts>=?2 AND ts<=?3 GROUP BY t ORDER BY t",
        ) else {
            return vec![];
        };
        st.query_map(params![b, from, to], |r| {
            Ok(Point {
                t: r.get(0)?,
                cpu: r.get(1)?,
                cpu_max: r.get(2)?,
                ram_used: r.get(3)?,
                ram_used_max: r.get(4)?,
                ram_avail_min: r.get(5)?,
                commit: r.get(6)?,
                gpu_util: r.get(7)?,
                gpu_util_max: r.get(8)?,
                vram_used: r.get(9)?,
                vram_used_max: r.get(10)?,
                gpu_temp: r.get(11)?,
                gpu_power: r.get(12)?,
                cpu_temp: r.get(13)?,
                disk_r: r.get(14)?,
                disk_w: r.get(15)?,
                net_rx: r.get(16)?,
                net_tx: r.get(17)?,
            })
        })
        .map(|i| i.flatten().collect())
        .unwrap_or_default()
    }

    /// Process snapshot nearest to `ts` (within `tol_ms`), if any.
    pub fn snap_at(&self, ts: i64, tol_ms: i64) -> Option<SnapAt> {
        self.flush();
        let conn = self.reader();
        let before: Option<i64> = conn.query_row("SELECT MAX(ts) FROM proc_snaps WHERE ts<=?", [ts], |r| r.get(0)).ok().flatten();
        let after: Option<i64> = conn.query_row("SELECT MIN(ts) FROM proc_snaps WHERE ts>=?", [ts], |r| r.get(0)).ok().flatten();
        let pick = match (before, after) {
            (Some(b), Some(a)) => {
                if ts - b <= a - ts { b } else { a }
            }
            (Some(b), None) => b,
            (None, Some(a)) => a,
            _ => return None,
        };
        if (pick - ts).abs() > tol_ms {
            return None;
        }
        let mut st = conn.prepare("SELECT pid,name,pws,ws,commit_b,cpu,gpu,gpu_mem FROM proc_snaps WHERE ts=? ORDER BY pws DESC").ok()?;
        let procs: Vec<SnapProc> = st
            .query_map([pick], |r| {
                Ok(SnapProc { pid: r.get::<_, i64>(0)? as u32, name: r.get(1)?, private_ws: r.get::<_, i64>(2)? as u64, working_set: r.get::<_, i64>(3)? as u64, commit: r.get::<_, i64>(4)? as u64, cpu: r.get(5)?, gpu: r.get(6)?, gpu_mem: r.get::<_, Option<i64>>(7)?.map(|v| v as u64) })
            })
            .ok()?
            .flatten()
            .collect();
        let mut agg: HashMap<String, AppAgg> = HashMap::new();
        for p in &procs {
            let e = agg.entry(p.name.to_ascii_lowercase()).or_insert(AppAgg { name: p.name.clone(), count: 0, private_ws: 0, cpu: 0.0, gpu_mem: None });
            e.count += 1;
            e.private_ws += p.private_ws;
            e.cpu += p.cpu;
            if let Some(m) = p.gpu_mem {
                *e.gpu_mem.get_or_insert(0) += m;
            }
        }
        let mut apps: Vec<AppAgg> = agg.into_values().collect();
        apps.sort_by(|a, b| b.private_ws.cmp(&a.private_ws));
        Some(SnapAt { ts: pick, requested: ts, procs, apps })
    }

    pub fn spikes(&self, from: i64, to: i64, limit: i64) -> Vec<SpikeRow> {
        let conn = self.reader();
        let Ok(mut st) = conn.prepare("SELECT id,ts,kind,value,baseline,summary,detail FROM spikes WHERE ts>=? AND ts<=? ORDER BY ts DESC LIMIT ?") else { return vec![] };
        st.query_map(params![from, to, limit], |r| {
            let d: String = r.get::<_, Option<String>>(6)?.unwrap_or_default();
            Ok(SpikeRow { id: r.get(0)?, ts: r.get(1)?, kind: r.get(2)?, value: r.get::<_, Option<f64>>(3)?.unwrap_or(0.0), baseline: r.get(4)?, summary: r.get::<_, Option<String>>(5)?.unwrap_or_default(), detail: serde_json::from_str(&d).unwrap_or(serde_json::Value::Null) })
        })
        .map(|i| i.flatten().collect())
        .unwrap_or_default()
    }

    /// Peak memory per executable (instances summed per snapshot) over a window.
    pub fn peaks(&self, from: i64, to: i64, limit: i64) -> Vec<PeakRow> {
        self.flush();
        let conn = self.reader();
        let Ok(mut st) = conn.prepare(
            "WITH agg AS (SELECT ts, lower(name) AS k, name, SUM(pws) AS tot, COUNT(*) AS n, SUM(cpu) AS cpu, SUM(gpu_mem) AS gm
                          FROM proc_snaps WHERE ts>=?1 AND ts<=?2 GROUP BY ts, lower(name))
             SELECT name, MAX(tot) AS peak,
                    (SELECT a2.ts FROM agg a2 WHERE a2.k=agg.k ORDER BY a2.tot DESC, a2.ts DESC LIMIT 1) AS peak_ts,
                    MAX(n), MAX(cpu), MAX(gm)
             FROM agg GROUP BY k ORDER BY peak DESC LIMIT ?3",
        ) else {
            return vec![];
        };
        st.query_map(params![from, to, limit], |r| {
            Ok(PeakRow { name: r.get(0)?, peak_private_ws: r.get::<_, i64>(1)? as u64, peak_ts: r.get(2)?, max_instances: r.get::<_, i64>(3)? as u32, peak_cpu: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0), peak_gpu_mem: r.get::<_, Option<i64>>(5)?.map(|v| v as u64) })
        })
        .map(|i| i.flatten().collect())
        .unwrap_or_default()
    }

    /// Private working set over time for one executable (summed instances).
    pub fn app_history(&self, name: &str, from: i64, to: i64) -> Vec<(i64, u64)> {
        self.flush();
        let conn = self.reader();
        let Ok(mut st) = conn.prepare("SELECT ts, SUM(pws) FROM proc_snaps WHERE lower(name)=lower(?) AND ts>=? AND ts<=? GROUP BY ts ORDER BY ts") else { return vec![] };
        st.query_map(params![name, from, to], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u64))).map(|i| i.flatten().collect()).unwrap_or_default()
    }

    /// Processes whose private working set grew steadily over the last `window_min` minutes.
    pub fn growth(&self, window_min: i64) -> Vec<GrowthRow> {
        self.flush();
        let conn = self.reader();
        let now = crate::monitor::now_ms() as i64;
        let from = now - window_min * 60_000;
        let Ok(mut st) = conn.prepare("SELECT pid, ct, name, ts, pws FROM proc_snaps WHERE ts>=? ORDER BY pid, ct, ts") else { return vec![] };
        let rows: Vec<(u32, i64, String, i64, u64)> = st
            .query_map([from], |r| Ok((r.get::<_, i64>(0)? as u32, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? as u64)))
            .map(|i| i.flatten().collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        let mut i = 0;
        while i < rows.len() {
            let mut j = i;
            while j < rows.len() && rows[j].0 == rows[i].0 && rows[j].1 == rows[i].1 {
                j += 1;
            }
            let series = &rows[i..j];
            i = j;
            if series.len() < 12 {
                continue;
            }
            let span_min = (series.last().unwrap().3 - series[0].3) as f64 / 60_000.0;
            if span_min < 15.0 || now - series.last().unwrap().3 > 180_000 {
                continue; // too short, or the process has exited / stopped being recorded
            }
            let start = series[0].4 as f64;
            let end = series.last().unwrap().4 as f64;
            let growth = end - start;
            if growth < 300.0 * 1048576.0 || end < start * 1.25 {
                continue;
            }
            let mut ups = 0;
            let mut steps = 0;
            let mut max_drop = 0.0f64;
            let mut peak = series[0].4 as f64;
            for w in series.windows(2) {
                steps += 1;
                if w[1].4 >= w[0].4 {
                    ups += 1;
                }
                peak = peak.max(w[1].4 as f64);
                max_drop = max_drop.max((peak - w[1].4 as f64) / peak);
            }
            if (ups as f64) / (steps as f64) < 0.7 || max_drop > 0.2 {
                continue;
            }
            out.push(GrowthRow {
                pid: series[0].0,
                name: series[0].2.clone(),
                start_ws: series[0].4,
                now_ws: series.last().unwrap().4,
                growth_per_hour: growth / (span_min / 60.0),
                minutes: span_min,
                samples: series.len() as u32,
                note: "Sustained memory growth observed. This alone does not prove a leak — caches and loaded documents also grow.".to_string(),
            });
        }
        out.sort_by(|a, b| b.growth_per_hour.partial_cmp(&a.growth_per_hour).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    pub fn prune(&self, retention_days: u32) {
        let mut g = self.inner.lock();
        Self::flush_locked(&mut g);
        let cutoff = crate::monitor::now_ms() as i64 - retention_days as i64 * 86_400_000;
        for q in ["DELETE FROM samples WHERE ts<?", "DELETE FROM proc_snaps WHERE ts<?", "DELETE FROM spikes WHERE ts<?", "DELETE FROM alerts WHERE ts<?"] {
            let _ = g.conn.execute(q, [cutoff]);
        }
        let _ = g.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    pub fn clear(&self) {
        let mut g = self.inner.lock();
        g.samples.clear();
        g.snaps.clear();
        let _ = g.conn.execute_batch("DELETE FROM samples; DELETE FROM proc_snaps; DELETE FROM spikes; DELETE FROM alerts; VACUUM;");
    }

    pub fn info(&self) -> StoreInfo {
        self.flush();
        let conn = self.reader();
        let cnt = |t: &str| conn.query_row(&format!("SELECT COUNT(*) FROM {}", t), [], |r| r.get::<_, i64>(0)).unwrap_or(0);
        let oldest = conn.query_row("SELECT MIN(ts) FROM samples", [], |r| r.get::<_, Option<i64>>(0)).optional().ok().flatten().flatten();
        let newest = conn.query_row("SELECT MAX(ts) FROM samples", [], |r| r.get::<_, Option<i64>>(0)).optional().ok().flatten().flatten();
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0) + std::fs::metadata(self.path.with_extension("db-wal")).map(|m| m.len()).unwrap_or(0);
        StoreInfo { path: self.path.to_string_lossy().into_owned(), size_bytes: size, samples: cnt("samples"), snapshots: cnt("proc_snaps"), spikes: cnt("spikes"), oldest_ts: oldest, newest_ts: newest }
    }

    pub fn export_csv(&self, from: i64, to: i64, path: &str) -> Result<usize, String> {
        use std::io::Write;
        self.flush();
        let conn = self.reader();
        let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
        writeln!(f, "timestamp_utc,unix_ms,cpu_pct,ram_used_bytes,ram_available_bytes,commit_bytes,gpu_util_pct,vram_used_bytes,gpu_temp_c,gpu_power_w,cpu_temp_c,disk_read_Bps,disk_write_Bps,net_rx_Bps,net_tx_Bps").map_err(|e| e.to_string())?;
        let mut st = conn.prepare("SELECT ts,cpu,ram_used,ram_avail,commit_b,gpu_util,vram_used,gpu_temp,gpu_power,cpu_temp,disk_r,disk_w,net_rx,net_tx FROM samples WHERE ts>=? AND ts<=? ORDER BY ts").map_err(|e| e.to_string())?;
        let mut rows = st.query(params![from, to]).map_err(|e| e.to_string())?;
        let mut n = 0;
        let fmt = |v: Option<f64>| v.map(|x| format!("{:.2}", x)).unwrap_or_default();
        while let Some(r) = rows.next().map_err(|e| e.to_string())? {
            let ts: i64 = r.get(0).map_err(|e| e.to_string())?;
            let vals: Vec<String> = (1..14).map(|i| fmt(r.get::<_, Option<f64>>(i).ok().flatten())).collect();
            writeln!(f, "{},{},{}", crate::iso_utc(ts), ts, vals.join(",")).map_err(|e| e.to_string())?;
            n += 1;
        }
        Ok(n)
    }
}
