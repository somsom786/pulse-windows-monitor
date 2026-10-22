import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import type { App, Diff, DupGroup, DupProgress, Location, Overview, Row, ScanMeta } from "../lib/types";
import { ago, bytes, CAT_COLORS, CAT_NAMES, dateTime, num } from "../lib/format";
import { Badge, Bar, Btn, cx, Empty, Spinner } from "../components/ui";
import { FileTable } from "../components/FileTable";
import { useFileActions } from "../components/menu";
import { useStore } from "../lib/store";

type Actions = ReturnType<typeof useFileActions>;

export function Insights({ tab, meta, overview, scans, metric, onShow, rowMenu, actions, startScan, onFilterExt }: { tab: string; meta: ScanMeta; overview: Overview | null; scans: ScanMeta[]; metric: "alloc" | "logical"; onShow: (path: string) => void; rowMenu: (e: React.MouseEvent, r: Row) => void; actions: Actions; startScan: (r: string[]) => void; onFilterExt: (ext: string) => void }) {
  const load = !overview && (tab === "files" || tab === "folders" || tab === "types" || tab === "coverage");
  if (load) {
    return (
      <div className="py-10 flex justify-center">
        <Spinner />
      </div>
    );
  }
  switch (tab) {
    case "files":
      return <FileTable rows={overview?.topFiles ?? []} metric={metric} showPath onMenu={rowMenu} maxHeight={460} onOpen={(r) => onShow(r.path)} />;
    case "folders":
      return (
        <div>
          <p className="px-4 py-2 text-[12px] text-muted border-b border-line">Folders that hold a lot of data themselves, rather than just containing one huge subfolder. Double-click a row, or use the menu, to jump to it on the map.</p>
          <FileTable rows={overview?.topFolders ?? []} metric={metric} showPath onMenu={(e, r) => rowMenu(e, r)} onOpen={(r) => onShow(r.path)} maxHeight={460} />
        </div>
      );
    case "types":
      return <Types overview={overview!} onFilterExt={onFilterExt} />;
    case "cleanup":
      return <Cleanup onShow={onShow} actions={actions} startScan={startScan} />;
    case "apps":
      return <Apps />;
    case "installers":
      return <Installers rowMenu={rowMenu} onShow={onShow} metric={metric} />;
    case "dups":
      return <Dups actions={actions} />;
    case "changes":
      return <Growth scans={scans} meta={meta} />;
    case "coverage":
      return <Coverage overview={overview!} meta={meta} />;
  }
  return null;
}

function Types({ overview, onFilterExt }: { overview: Overview; onFilterExt: (e: string) => void }) {
  const total = overview.meta.totalAlloc || 1;
  const cats = [...overview.cats].filter((c) => c.alloc > 0).sort((a, b) => b.alloc - a.alloc);
  return (
    <div className="grid lg:grid-cols-2 gap-6 p-4">
      <div>
        <div className="text-[11px] uppercase tracking-wide text-muted mb-2">By category</div>
        <div className="space-y-2">
          {cats.map((c) => (
            <div key={c.cat}>
              <div className="flex justify-between text-[12.5px]">
                <span className="flex items-center gap-2">
                  <span className="w-2.5 h-2.5 rounded-sm" style={{ background: CAT_COLORS[c.cat] }} />
                  {c.name}
                </span>
                <span className="num text-muted">
                  {bytes(c.alloc)} · {((c.alloc * 100) / total).toFixed(1)}% · {num(c.count)} files
                </span>
              </div>
              <Bar value={c.alloc} max={cats[0].alloc} tone={CAT_COLORS[c.cat]} className="mt-1" />
            </div>
          ))}
        </div>
      </div>
      <div>
        <div className="text-[11px] uppercase tracking-wide text-muted mb-2">Top extensions · click to filter</div>
        <div className="max-h-[420px] overflow-auto">
          <table className="w-full text-[12.5px]">
            <tbody>
              {overview.exts.map((e) => (
                <tr key={e.ext} className="border-b border-line/60 hover:bg-surface2 cursor-pointer" onClick={() => onFilterExt(e.ext)}>
                  <td className="py-1.5 px-2 font-mono text-[12px]">
                    <span className="inline-block w-2 h-2 rounded-sm mr-2" style={{ background: CAT_COLORS[e.cat] }} />.{e.ext}
                  </td>
                  <td className="px-2 text-right num">{bytes(e.alloc)}</td>
                  <td className="px-2 text-right num text-muted">{num(e.count)} files</td>
                  <td className="px-2 text-muted hidden xl:table-cell">{CAT_NAMES[e.cat]}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

const RISK: Record<string, { tone: "good" | "warn" | "neutral"; label: string }> = {
  safe: { tone: "good", label: "Safe to clean" },
  review: { tone: "warn", label: "Review first" },
  system: { tone: "neutral", label: "Protected system data" },
};

function Cleanup({ onShow, actions, startScan }: { onShow: (p: string) => void; actions: Actions; startScan: (r: string[]) => void }) {
  const [locs, setLocs] = useState<Location[] | null>(null);
  const [exp, setExp] = useState<string | null>(null);
  const [kids, setKids] = useState<Record<string, Row[]>>({});
  useEffect(() => {
    api.storageLocations().then(setLocs).catch(() => setLocs([]));
  }, []);
  if (!locs) return <div className="py-10 flex justify-center"><Spinner /></div>;
  const groups = ["Downloads", "Temporary", "Caches", "Games", "AppData", "System"];
  const toggle = async (l: Location) => {
    const key = l.path;
    if (exp === key) return setExp(null);
    setExp(key);
    if (!kids[key] && (l.group === "Games" || l.group === "AppData" || l.group === "Downloads" || l.group === "Caches")) {
      const r = await api.storageLocChildren(l.path);
      setKids((k) => ({ ...k, [key]: r }));
    }
  };
  return (
    <div className="p-4 space-y-5">
      <div className="flex flex-wrap gap-2 items-center rounded-lg bg-surface2 border border-line px-3 py-2">
        <span className="text-[12.5px] text-muted mr-1">Windows-native cleanup:</span>
        <Btn small onClick={() => api.system("ms-settings:storagesense").catch((e) => actions.run(Promise.reject(e)))}>Storage settings</Btn>
        <Btn small onClick={() => api.system("ms-settings:appsfeatures").catch(() => {})}>Installed apps</Btn>
        <Btn small onClick={() => api.system("cleanmgr").catch(() => {})}>Disk Cleanup</Btn>
        <span className="text-[12px] text-faint">PULSE never deletes automatically. Items below tell you what they are and the safest way to clean them.</span>
      </div>
      {groups.map((g) => {
        const items = locs.filter((l) => l.group === g);
        if (!items.length) return null;
        return (
          <div key={g}>
            <div className="text-[11px] uppercase tracking-wide text-muted mb-1.5">{g === "Temporary" ? "Temporary & system caches" : g === "Games" ? "Game libraries" : g === "AppData" ? "AppData & user data" : g}</div>
            <div className="rounded-lg border border-line divide-y divide-line">
              {items.map((l) => (
                <div key={l.path}>
                  <div className="flex items-center gap-3 px-3 py-2 hover:bg-surface2 cursor-pointer" onClick={() => toggle(l)}>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{l.label}</span>
                        <Badge tone={RISK[l.risk].tone}>{RISK[l.risk].label}</Badge>
                      </div>
                      <div className="text-[11.5px] text-faint truncate">{l.path}</div>
                    </div>
                    <div className="text-right num">
                      <div className="font-semibold">{bytes(l.alloc)}</div>
                      <div className="text-[11px] text-muted">{num(l.files)} files</div>
                    </div>
                  </div>
                  {exp === l.path && (
                    <div className="px-3 pb-3 pt-1 bg-surface2/50 space-y-2">
                      <div className="text-[12.5px]"><span className="text-muted">What it is: </span>{l.what}</div>
                      {l.cleanup && <div className="text-[12.5px]"><span className="text-muted">Safe route: </span>{l.cleanup}</div>}
                      <div className="flex gap-2 flex-wrap">
                        <Btn small onClick={() => onShow(l.path)}>Show on map</Btn>
                        <Btn small onClick={() => actions.run(api.openFolder(l.path))}>Open in Explorer</Btn>
                        <Btn small onClick={() => actions.copy(l.path)}>Copy path</Btn>
                        <Btn small kind="ghost" onClick={() => startScan([l.path])}>Rescan</Btn>
                      </div>
                      {kids[l.path]?.length ? (
                        <div className="mt-1">
                          <div className="text-[11px] uppercase tracking-wide text-muted mb-1">Largest subfolders</div>
                          {kids[l.path].slice(0, 12).map((k) => (
                            <div key={k.id} className="flex justify-between text-[12.5px] py-0.5 hover:bg-surface3 rounded px-1 cursor-pointer" onClick={() => onShow(k.path)}>
                              <span className="truncate">{k.name}</span>
                              <span className="num text-muted">{bytes(k.alloc)}</span>
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        );
      })}
      {locs.length === 0 && <Empty title="None of the usual cleanup locations are inside this scan" body="Scan the whole C: drive to see Downloads, caches, temp folders and game libraries." />}
    </div>
  );
}

function Apps() {
  const [apps, setApps] = useState<App[] | null>(null);
  useEffect(() => {
    api.storageApps().then(setApps).catch(() => setApps([]));
  }, []);
  if (!apps) return <div className="py-10 flex justify-center"><Spinner /></div>;
  return (
    <div>
      <p className="px-4 py-2 text-[12px] text-muted border-b border-line">Read from the Windows uninstall registry. "Measured" sizes come from your scan of the install folder; "installer estimate" is what the installer reported and can be inaccurate or missing. Uninstall through Windows Settings.</p>
      <div className="max-h-[460px] overflow-auto">
        <table className="w-full text-[12.5px]">
          <thead>
            <tr className="text-left text-muted text-[11px] uppercase tracking-wide">
              <th className="sticky top-0 bg-surface px-3 py-2">Application</th>
              <th className="sticky top-0 bg-surface px-3 py-2">Publisher</th>
              <th className="sticky top-0 bg-surface px-3 py-2 text-right">Size</th>
              <th className="sticky top-0 bg-surface px-3 py-2">Source</th>
            </tr>
          </thead>
          <tbody>
            {apps.map((a, i) => (
              <tr key={i} className="border-b border-line/60 hover:bg-surface2">
                <td className="px-3 py-1.5">
                  <div>{a.name}</div>
                  {a.location && <div className="text-[11px] text-faint truncate max-w-[520px]">{a.location}</div>}
                </td>
                <td className="px-3 py-1.5 text-muted">{a.publisher}</td>
                <td className="px-3 py-1.5 text-right num">{a.size ? bytes(a.size) : "—"}</td>
                <td className="px-3 py-1.5">{a.source === "measured" ? <Badge tone="good">measured</Badge> : a.source === "unknown" ? <Badge>unknown</Badge> : <Badge tone="warn">installer estimate</Badge>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function Installers({ rowMenu, onShow, metric }: { rowMenu: (e: React.MouseEvent, r: Row) => void; onShow: (p: string) => void; metric: "alloc" | "logical" }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  useEffect(() => {
    api.storageInstallers().then(setRows).catch(() => setRows([]));
  }, []);
  if (!rows) return <div className="py-10 flex justify-center"><Spinner /></div>;
  const total = rows.reduce((a, r) => a + r.alloc, 0);
  return (
    <div>
      <p className="px-4 py-2 text-[12px] text-muted border-b border-line">Installers and archives (20 MB+) that have not been modified in 60+ days, excluding system and program folders. Total: <b className="text-ink">{bytes(total)}</b>. Review before deleting — an archive may be your only copy.</p>
      <FileTable rows={rows} metric={metric} showPath onMenu={rowMenu} onOpen={(r) => onShow(r.path)} maxHeight={460} empty="No old installers or archives found." />
    </div>
  );
}

function Dups({ actions }: { actions: Actions }) {
  const [res, setRes] = useState<DupGroup[] | null>(null);
  const [running, setRunning] = useState(false);
  const [prog, setProg] = useState<DupProgress | null>(null);
  const [minMb, setMinMb] = useState("5");
  const toast = useStore((s) => s.toast);
  useEffect(() => {
    api.dupResult().then(setRes).catch(() => {});
    const u = [
      listen<DupProgress>("dup-progress", (e) => setProg(e.payload)),
      listen<boolean>("dup-done", async (e) => {
        setRunning(false);
        if (!e.payload) setRes(await api.dupResult());
      }),
    ];
    return () => u.forEach((p) => p.then((f) => f()));
  }, []);
  const wasted = (res ?? []).reduce((a, g) => a + g.wasted, 0);
  return (
    <div>
      <div className="px-4 py-3 border-b border-line flex items-center gap-3 flex-wrap">
        <span className="text-[12.5px] text-muted">Finds files with identical content (size match, then BLAKE3 hash). Reads file contents, so it can take a while on large drives. Windows and system folders are skipped.</span>
        <span className="flex-1" />
        <label className="text-[12px] text-muted flex items-center gap-1.5">
          Minimum size
          <input type="number" min={1} className="w-16" value={minMb} onChange={(e) => setMinMb(e.target.value)} disabled={running} /> MB
        </label>
        {running ? (
          <Btn kind="danger" onClick={() => api.dupCancel()}>
            Cancel
          </Btn>
        ) : (
          <Btn
            kind="primary"
            onClick={async () => {
              try {
                setRes(null);
                setProg(null);
                setRunning(true);
                await api.dupStart(Math.max(1, parseInt(minMb) || 1));
              } catch (e) {
                setRunning(false);
                toast({ kind: "bad", title: "Could not start", body: String(e) });
              }
            }}
          >
            Find duplicates
          </Btn>
        )}
      </div>
      {running && (
        <div className="px-4 py-4">
          <div className="flex items-center gap-3 text-[12.5px]">
            <Spinner /> {prog?.stage ?? "Preparing…"} {prog && prog.total > 0 && <span className="num text-muted">{num(prog.done)} / {num(prog.total)}</span>}
          </div>
          {prog && prog.total > 0 && <Bar value={prog.done} max={prog.total} className="mt-2" />}
        </div>
      )}
      {!running && res && (
        <div className="max-h-[460px] overflow-auto">
          <div className="px-4 py-2 text-[12.5px] text-muted">
            {res.length} duplicate groups · <b className="text-ink">{bytes(wasted)}</b> potentially reclaimable (keeping one copy of each).
          </div>
          {res.map((g, i) => (
            <div key={i} className="px-4 py-2 border-t border-line/60">
              <div className="flex justify-between text-[12.5px] mb-1">
                <span className="font-medium">
                  {g.files.length} copies × {bytes(g.size)}
                </span>
                <span className="text-warn num">{bytes(g.wasted)} wasted</span>
              </div>
              {g.files.map((f) => (
                <div key={f.id} className="flex items-center gap-2 text-[12px] py-0.5 group">
                  <span className="truncate flex-1 text-muted" title={f.path}>{f.path}</span>
                  <span className="text-faint">{ago(f.mtime)}</span>
                  <button className="text-muted hover:text-ink px-1" onClick={() => actions.run(api.reveal(f.path))}>Show</button>
                  <button className="text-bad/80 hover:text-bad px-1" onClick={() => actions.recycle([f.path], f.alloc)}>Recycle…</button>
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
      {!running && !res && <Empty title="No duplicate scan yet" body="This is optional and separate from the disk scan. Raise the minimum size to make it faster." />}
    </div>
  );
}

function Growth({ scans, meta }: { scans: ScanMeta[]; meta: ScanMeta }) {
  const same = scans.filter((s) => s.root === meta.root).sort((a, b) => a.startedUnix - b.startedUnix);
  const [a, setA] = useState<string>("");
  const [diff, setDiff] = useState<Diff | null>(null);
  const [busy, setBusy] = useState(false);
  const others = same.filter((s) => s.id !== meta.id);
  useEffect(() => {
    if (!a && others.length) setA(others[others.length - 1].id);
  }, [others.length]);
  if (same.length < 2) {
    return <Empty title="Growth needs two scans of the same location" body={`Run another scan of ${meta.root} later. PULSE keeps each saved scan so it can show which folders grew or shrank between them.`} />;
  }
  return (
    <div>
      <div className="px-4 py-3 flex items-center gap-3 border-b border-line flex-wrap">
        <span className="text-[12.5px] text-muted">Compare the open scan ({ago(meta.startedUnix)}) with</span>
        <select value={a} onChange={(e) => setA(e.target.value)}>
          {others.map((s) => (
            <option key={s.id} value={s.id}>
              {dateTime(s.startedUnix * 1000)} — {bytes(s.totalAlloc)}
            </option>
          ))}
        </select>
        <Btn
          kind="primary"
          disabled={!a || busy}
          onClick={async () => {
            setBusy(true);
            try {
              setDiff(await api.storageDiff(a, meta.id));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "Comparing…" : "Compare"}
        </Btn>
      </div>
      {diff && (
        <div className="max-h-[460px] overflow-auto">
          <div className="px-4 py-2 text-[12.5px] text-muted">
            Total changed from <b className="text-ink">{bytes(diff.totalBefore)}</b> to <b className="text-ink">{bytes(diff.totalAfter)}</b> ({diff.totalAfter >= diff.totalBefore ? "+" : "−"}
            {bytes(Math.abs(diff.totalAfter - diff.totalBefore))}). Showing folders (down to 7 levels) that changed by 50 MB or more; parent folders include their children's changes.
          </div>
          <table className="w-full text-[12.5px]">
            <tbody>
              {diff.rows.map((r) => (
                <tr key={r.path} className="border-t border-line/60 hover:bg-surface2">
                  <td className="px-4 py-1.5 truncate max-w-0" title={r.path}>
                    {r.path}
                  </td>
                  <td className="px-2 text-right num text-muted w-24">{bytes(r.before)}</td>
                  <td className="px-2 text-right num w-24">{bytes(r.after)}</td>
                  <td className={cx("px-4 text-right num w-28 font-medium", r.delta > 0 ? "text-warn" : "text-good")}>
                    {r.delta > 0 ? "+" : "−"}
                    {bytes(Math.abs(r.delta))}
                  </td>
                </tr>
              ))}
              {diff.rows.length === 0 && (
                <tr>
                  <td className="px-4 py-6 text-center text-muted">No folder changed by 50 MB or more.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function Coverage({ overview, meta }: { overview: Overview; meta: ScanMeta }) {
  const u = overview.unaccounted;
  const used = meta.volumeTotal - meta.volumeFree;
  const parts = useMemo(
    () => [
      { l: "Found by scan", v: u.scannedAlloc, c: "var(--accent)" },
      { l: "Not attributed", v: Math.max(0, u.gap), c: "var(--warn)" },
      { l: "Free", v: meta.volumeFree, c: "var(--surface-3)" },
    ],
    [u, meta],
  );
  return (
    <div className="p-4 space-y-4">
      {u.coversWholeVolume ? (
        <>
          <div>
            <div className="flex h-3 rounded-full overflow-hidden bg-surface3">
              {parts.map((p) => (
                <div key={p.l} style={{ width: `${(p.v * 100) / meta.volumeTotal}%`, background: p.c }} title={`${p.l}: ${bytes(p.v)}`} />
              ))}
            </div>
            <div className="flex gap-5 mt-2 text-[12.5px] flex-wrap">
              {parts.map((p) => (
                <span key={p.l} className="flex items-center gap-1.5">
                  <span className="w-2.5 h-2.5 rounded-sm border border-line2" style={{ background: p.c }} />
                  {p.l} <b className="num">{bytes(p.v)}</b>
                </span>
              ))}
            </div>
          </div>
          <div className="text-[13px]">
            Windows reports <b className="num">{bytes(used)}</b> used on this drive; the scan found <b className="num">{bytes(u.scannedAlloc)}</b>. {u.gap > 0 ? <>The remaining <b className="num text-warn">{bytes(u.gap)}</b> could not be attributed to files.</> : <>The scan found as much as Windows reports.</>}
          </div>
        </>
      ) : (
        <div className="text-[13px] text-muted">This is a folder scan, so it is not compared with the whole drive.</div>
      )}
      <ul className="list-disc pl-5 space-y-1.5 text-[12.5px] text-muted">
        {u.notes.map((n, i) => (
          <li key={i}>{n}</li>
        ))}
        <li>Logical sizes only count file contents. Space on disk includes cluster rounding. Very small files stored inside the NTFS master file table and alternate data streams are not itemized by any directory scan.</li>
      </ul>
      {overview.errorSamples.length > 0 && (
        <div>
          <div className="text-[11px] uppercase tracking-wide text-muted mb-1">Folders that could not be read ({meta.errors})</div>
          <div className="max-h-48 overflow-auto rounded-lg border border-line divide-y divide-line/60">
            {overview.errorSamples.map((e, i) => (
              <div key={i} className="flex justify-between gap-3 px-3 py-1 text-[12px]">
                <span className="truncate" title={e.path}>{e.path}</span>
                <span className="text-faint num shrink-0">{e.code === 5 ? "Access denied" : e.code === 32 ? "In use" : `Error ${e.code}`}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
