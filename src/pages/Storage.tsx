import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { Children, Crumb, Drive, Filter, Overview, Row, ScanMeta, SearchResult, Tm } from "../lib/types";
import { ago, bytes, CAT_COLORS, CAT_NAMES, dur, num, pct } from "../lib/format";
import { Badge, Bar, Btn, Card, cx, Empty, Icon, Info, Segmented, Spinner, Tabs, useDebounced } from "../components/ui";
import { Treemap, TmPick } from "../components/Treemap";
import { FileTable, SortKey } from "../components/FileTable";
import { useFileActions, useMenu } from "../components/menu";
import { Insights } from "./StorageInsights";

type Metric = "alloc" | "logical";

export function Storage() {
  const { scanning, scanProgress, scanSaving, scanBatch, scanMeta, scanVersion, setScan, toast } = useStore();
  const [drives, setDrives] = useState<Drive[]>([]);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [scans, setScans] = useState<ScanMeta[]>([]);
  const [loadingScan, setLoadingScan] = useState(false);
  const [node, setNode] = useState(0);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [tm, setTm] = useState<Tm | null>(null);
  const [children, setChildren] = useState<Children | null>(null);
  const [metric, setMetric] = useState<Metric>("alloc");
  const [catFilter, setCatFilter] = useState<number | null>(null);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [tab, setTab] = useState("browse");
  const [sort, setSort] = useState<SortKey>("size");
  const [desc, setDesc] = useState(true);
  const [search, setSearch] = useState<{ name: string; exts: string; cat: number | "all"; minMb: string; kind: "all" | "files" | "dirs"; olderDays: string }>({ name: "", exts: "", cat: "all", minMb: "", kind: "all", olderDays: "" });
  const [results, setResults] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const menu = useMenu();
  const loadedFor = useRef<string | null>(null);

  const actions = useFileActions(() => {
    refreshView(node);
  });

  const meta = scanMeta;
  const refreshScans = useCallback(() => api.scanList().then(setScans).catch(() => {}), []);

  useEffect(() => {
    api.drives().then(setDrives).catch(() => {});
    refreshScans();
  }, [refreshScans, scanVersion]);

  // Open the most recent saved scan once (fast reopen); never starts a scan.
  useEffect(() => {
    if (meta || scanning || loadedFor.current) return;
    (async () => {
      const st = await api.scanState();
      const list = await api.scanList();
      const id = st.current ?? list[0]?.id;
      if (!id) return;
      loadedFor.current = id;
      setLoadingScan(true);
      try {
        const m = st.loaded && st.current ? list.find((x) => x.id === st.current) ?? (await api.scanOpen(id)) : await api.scanOpen(id);
        setScan({ scanMeta: m });
      } catch (e) {
        toast({ kind: "bad", title: "Could not open saved scan", body: String(e) });
      } finally {
        setLoadingScan(false);
      }
    })();
  }, [meta, scanning, setScan, toast]);

  const refreshView = useCallback(
    async (id: number) => {
      if (!meta) return;
      try {
        const [t, ch, cr] = await Promise.all([api.treeTreemap(id, metric, 3), api.treeChildren(id, metric, 2000), api.treeCrumbs(id)]);
        setTm(t);
        setChildren(ch);
        setCrumbs(cr);
      } catch (e) {
        toast({ kind: "bad", title: "Could not read scan", body: String(e) });
      }
    },
    [meta, metric, toast],
  );

  useEffect(() => {
    if (!meta) return;
    setNode(0);
    setOverview(null);
    setResults(null);
    api.storageOverview().then(setOverview).catch(() => {});
    refreshView(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meta?.id]);

  useEffect(() => {
    if (meta) refreshView(node);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [node, metric]);

  // filtered search
  const dq = useDebounced(search, 300);
  const filterActive = !!(dq.name || dq.exts || dq.cat !== "all" || dq.minMb || dq.kind !== "all" || dq.olderDays);
  useEffect(() => {
    if (!meta || !filterActive) {
      setResults(null);
      return;
    }
    const f: Filter = {
      name: dq.name || undefined,
      exts: dq.exts ? dq.exts.split(/[ ,;]+/).filter(Boolean) : undefined,
      cats: dq.cat !== "all" ? [dq.cat] : undefined,
      minSize: dq.minMb ? Math.max(0, parseFloat(dq.minMb) || 0) * 1048576 : undefined,
      kind: dq.kind,
      olderThanDays: dq.olderDays ? parseInt(dq.olderDays) || undefined : undefined,
      folder: node,
      metric,
      sort: sort === "size" ? "size" : sort === "name" ? "name" : sort === "mtime" ? "mtime" : sort === "files" ? "files" : "size",
      desc,
      limit: 500,
    };
    setSearching(true);
    api
      .treeSearch(f)
      .then(setResults)
      .catch((e) => toast({ kind: "bad", title: "Search failed", body: String(e) }))
      .finally(() => setSearching(false));
  }, [dq, node, metric, sort, desc, meta?.id, filterActive, toast]);

  const startScan = async (roots: string[]) => {
    if (!roots.length) return;
    try {
      setScan({ scanning: true, scanProgress: null });
      await api.scanStart(roots);
    } catch (e) {
      setScan({ scanning: false });
      toast({ kind: "bad", title: "Could not start scan", body: String(e) });
    }
  };

  const pickFolder = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Choose a folder to scan" });
    if (typeof dir === "string") startScan([dir]);
  };

  const drill = (id: number) => {
    setCatFilter(null);
    setNode(id);
  };

  const rootPath = crumbs.length ? crumbs.map((c, i) => (i === 0 ? c.name.replace(/\\$/, "") : c.name)).join("\\") + (crumbs.length === 1 ? "\\" : "") : meta?.root ?? "";
  const currentPath = children?.node.path ?? rootPath;

  const rowMenu = (e: React.MouseEvent, r: Row) => {
    const extra = r.isDir ? [{ label: "Zoom into folder", onClick: () => drill(r.id) }, { label: "Rescan this folder", onClick: () => startScan([r.path]) }] : r.ext ? [{ label: `Filter to .${r.ext} files`, onClick: () => setSearch((s) => ({ ...s, exts: r.ext })) }] : [];
    const items = actions.itemsFor(r.path, r.isDir, extra, r.alloc);
    menu.open(e, items);
  };
  const tmMenu = (e: React.MouseEvent, p: TmPick) => {
    const extra = p.isDir ? [{ label: "Zoom into folder", onClick: () => drill(p.id) }, { label: "Rescan this folder", onClick: () => startScan([p.path]) }] : [];
    menu.open(e, actions.itemsFor(p.path, p.isDir, extra));
  };

  const sortedRows = useMemo(() => {
    if (filterActive) return results?.rows ?? [];
    const rows = [...(children?.rows ?? [])];
    const dir = desc ? -1 : 1;
    const v = (r: Row) => (metric === "alloc" ? r.alloc : r.size);
    rows.sort((a, b) => {
      switch (sort) {
        case "name":
          return dir * a.name.localeCompare(b.name);
        case "files":
          return dir * (a.files - b.files);
        case "mtime":
          return dir * (a.mtime - b.mtime);
        case "logical":
          return dir * ((metric === "alloc" ? a.size : a.alloc) - (metric === "alloc" ? b.size : b.alloc));
        case "type":
          return dir * (a.ext.localeCompare(b.ext) || a.cat - b.cat);
        default:
          return dir * (v(a) - v(b));
      }
    });
    return rows;
  }, [children, results, filterActive, sort, desc, metric]);

  const onSort = (k: SortKey) => {
    if (sort === k) setDesc(!desc);
    else {
      setSort(k);
      setDesc(k !== "name");
    }
  };

  // ---------- render ----------
  const idle = !scanning;
  return (
    <div className="h-full overflow-auto p-5 space-y-4">
      {actions.node}
      {menu.node}
      <div className="flex items-end justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-[20px] font-semibold tracking-tight">Storage Explorer</h1>
          <p className="text-muted text-[12.5px] mt-0.5">See exactly what fills your drives. Scans are run on demand and saved locally for instant reopening.</p>
        </div>
        {meta && (
          <div className="flex items-center gap-2">
            <Segmented
              value={metric}
              onChange={setMetric}
              options={[
                { value: "alloc", label: "Space on disk", title: "Allocated clusters: what the file actually occupies on the drive" },
                { value: "logical", label: "File size", title: "Logical size: the length of the file content" },
              ]}
            />
            <Info text="Space on disk (allocated) is rounded up to the cluster size and is what frees up when you delete. File size (logical) is the data length shown in Explorer. Compressed, sparse and cloud-only files can differ greatly between the two." />
          </div>
        )}
      </div>

      <ScanPanel drives={drives} picked={picked} setPicked={setPicked} scans={scans} meta={meta} onScan={startScan} onFolder={pickFolder} idle={idle} loading={loadingScan} onOpen={async (id) => { setLoadingScan(true); try { const m = await api.scanOpen(id); setScan({ scanMeta: m }); } catch (e) { toast({ kind: "bad", title: "Open failed", body: String(e) }); } finally { setLoadingScan(false); } }} onDelete={async (id) => { await api.scanDelete(id); if (meta?.id === id) setScan({ scanMeta: null }); loadedFor.current = null; refreshScans(); }} />

      {scanning && <ScanProgressCard p={scanProgress} saving={scanSaving} batch={scanBatch} />}

      {!meta && !scanning && !loadingScan && (
        <Card>
          <Empty icon={<Icon.storage width={36} height={36} />} title="No scan yet" body="Choose a drive or folder above and press Scan. A full 2 TB system drive typically takes 1–2 minutes. Nothing is scanned automatically, and PULSE never deletes anything on its own." />
        </Card>
      )}
      {loadingScan && (
        <Card>
          <div className="flex items-center gap-3 py-6 justify-center text-muted">
            <Spinner /> Opening saved scan…
          </div>
        </Card>
      )}

      {meta && !scanning && (
        <>
          <SummaryStrip meta={meta} overview={overview} metric={metric} />

          <Card
            pad={false}
            title={
              <span className="flex items-center gap-2">
                Map
                <span className="normal-case tracking-normal font-normal text-faint">area = {metric === "alloc" ? "space on disk" : "file size"}</span>
              </span>
            }
            right={<Legend overview={overview} catFilter={catFilter} setCatFilter={setCatFilter} metric={metric} />}
          >
            <div className="px-4 pt-2 pb-3 space-y-2">
              <div className="flex items-center gap-1 flex-wrap text-[12.5px]">
                {crumbs.map((c, i) => (
                  <span key={c.id} className="flex items-center gap-1">
                    {i > 0 && <Icon.chevron className="text-faint" />}
                    <button className={cx("px-1.5 py-0.5 rounded hover:bg-surface3", i === crumbs.length - 1 ? "text-ink font-medium" : "text-muted hover:text-ink")} onClick={() => drill(c.id)}>
                      {c.name}
                    </button>
                  </span>
                ))}
                <span className="flex-1" />
                {children && (
                  <span className="text-muted num">
                    {bytes(metric === "alloc" ? children.node.alloc : children.node.size)} · {num(children.node.files)} files
                  </span>
                )}
                {node !== 0 && (
                  <Btn small kind="ghost" onClick={() => drill(crumbs.length > 1 ? crumbs[crumbs.length - 2].id : 0)}>
                    ↑ Up
                  </Btn>
                )}
                <Btn small kind="ghost" onClick={() => startScan([currentPath])} title="Scan this folder again">
                  Rescan folder
                </Btn>
              </div>
              {tm && tm.value > 0 ? (
                <Treemap data={tm} rootPath={rootPath} metric={metric} catFilter={catFilter} onDrill={drill} onContext={tmMenu} height={Math.max(300, Math.min(520, Math.round(window.innerHeight * 0.46)))} />
              ) : (
                <div className="h-[300px] flex items-center justify-center text-muted">{tm ? "This folder is empty (or contains no measurable data)." : <Spinner />}</div>
              )}
            </div>
          </Card>

          <Card pad={false}>
            <div className="px-4">
              <Tabs
                value={tab}
                onChange={setTab}
                tabs={[
                  { id: "browse", label: "Files & folders" },
                  { id: "files", label: "Largest files" },
                  { id: "folders", label: "Largest folders" },
                  { id: "types", label: "File types" },
                  { id: "cleanup", label: "Cleanup & locations" },
                  { id: "apps", label: "Installed apps" },
                  { id: "installers", label: "Old installers" },
                  { id: "dups", label: "Duplicates" },
                  { id: "changes", label: "Growth" },
                  { id: "coverage", label: "Coverage" },
                ]}
              />
            </div>
            {tab === "browse" ? (
              <div>
                <div className="flex items-center gap-2 flex-wrap px-4 py-3 border-b border-line">
                  <div className="relative">
                    <Icon.search className="absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
                    <input type="text" placeholder="Filter by name…" className="pl-8 w-52" value={search.name} onChange={(e) => setSearch({ ...search, name: e.target.value })} />
                  </div>
                  <input type="text" placeholder="Extensions (mp4, zip)" className="w-44" value={search.exts} onChange={(e) => setSearch({ ...search, exts: e.target.value })} />
                  <select value={String(search.cat)} onChange={(e) => setSearch({ ...search, cat: e.target.value === "all" ? "all" : parseInt(e.target.value) })}>
                    <option value="all">All categories</option>
                    {CAT_NAMES.map((n, i) => (
                      <option key={n} value={i}>
                        {n}
                      </option>
                    ))}
                  </select>
                  <input type="number" min={0} placeholder="Min MB" className="w-24" value={search.minMb} onChange={(e) => setSearch({ ...search, minMb: e.target.value })} />
                  <input type="number" min={0} placeholder="Older than (days)" className="w-36" value={search.olderDays} onChange={(e) => setSearch({ ...search, olderDays: e.target.value })} />
                  <select value={search.kind} onChange={(e) => setSearch({ ...search, kind: e.target.value as any })}>
                    <option value="all">Files & folders</option>
                    <option value="files">Files only</option>
                    <option value="dirs">Folders only</option>
                  </select>
                  {filterActive && (
                    <Btn small kind="ghost" onClick={() => setSearch({ name: "", exts: "", cat: "all", minMb: "", kind: "all", olderDays: "" })}>
                      Clear filters
                    </Btn>
                  )}
                  <span className="flex-1" />
                  <span className="text-muted text-[12px] num">
                    {filterActive ? (searching ? "Searching…" : `${num(results?.matched ?? 0)} matches in this folder tree · ${bytes(results?.matchedBytes ?? 0)}${(results?.matched ?? 0) > 500 ? " (showing top 500)" : ""}`) : children ? `${num(children.total)} items${children.total > 2000 ? " (showing largest 2,000)" : ""}` : ""}
                  </span>
                </div>
                <FileTable rows={sortedRows} metric={metric} showPath={filterActive} parentTotal={filterActive ? undefined : metric === "alloc" ? children?.node.alloc : children?.node.size} sort={sort} desc={desc} onSort={onSort} onOpen={(r) => drill(r.id)} onMenu={rowMenu} maxHeight={420} empty={filterActive ? "No files match these filters." : "This folder is empty."} />
              </div>
            ) : (
              <Insights tab={tab} meta={meta} overview={overview} scans={scans} metric={metric} onShow={(path) => showPath(path)} rowMenu={rowMenu} actions={actions} startScan={startScan} onFilterExt={(ext) => { setSearch((s) => ({ ...s, exts: ext === "(none)" ? "" : ext })); setTab("browse"); }} />
            )}
          </Card>
        </>
      )}
    </div>
  );

  async function showPath(path: string) {
    const id = await api.treeFind(path);
    if (id != null) {
      setCatFilter(null);
      setNode(id);
      window.scrollTo?.(0, 0);
    } else toast({ kind: "warn", title: "Not in this scan", body: path });
  }
}

function compact(n: number) {
  return n >= 1e6 ? (n / 1e6).toFixed(2) + "M" : n >= 1e4 ? Math.round(n / 1e3) + "K" : num(n);
}

function SummaryStrip({ meta, overview, metric }: { meta: ScanMeta; overview: Overview | null; metric: Metric }) {
  const used = meta.volumeTotal - meta.volumeFree;
  return (
    <div className="grid grid-cols-2 md:grid-cols-3 xl:grid-cols-6 gap-3">
      {[
        { l: "Scanned", v: meta.root, s: ago(meta.startedUnix) + " · took " + dur(meta.durationMs / 1000), tip: "The folder or drive this scan covers." },
        { l: "Space on disk", v: bytes(meta.totalAlloc), s: `File size ${bytes(meta.totalLogical)}`, tip: "Allocated clusters found by the scan." },
        { l: "Files / folders", v: `${compact(meta.files)} / ${compact(meta.dirs)}`, s: meta.fsName || "", tip: "Number of items traversed." },
        { l: "Drive used", v: bytes(used), s: `${bytes(meta.volumeFree)} free of ${bytes(meta.volumeTotal)}`, tip: "Reported by Windows for the whole volume." },
        { l: "Unexplained", v: overview ? (overview.unaccounted.coversWholeVolume ? bytes(Math.max(0, overview.unaccounted.gap)) : "—") : "…", s: overview?.unaccounted.coversWholeVolume ? "see Coverage tab" : "folder scan", tip: "Used space on the drive that the scan could not attribute to files: system metadata, restore points, reserved storage, or folders Windows denied access to." },
        { l: "Access errors", v: num(meta.errors), s: meta.errors ? "folders skipped" : "none", tip: "Folders that could not be opened (permissions or in use). Their contents are excluded from totals." },
      ].map((x) => (
        <div key={x.l} className="rounded-xl border border-line bg-surface px-4 py-3 min-w-0">
          <div className="text-[11px] text-muted uppercase tracking-wide flex items-center gap-1">
            {x.l}
            <Info text={x.tip} />
          </div>
          <div className="text-[16px] font-semibold num truncate" title={x.v}>
            {x.v}
          </div>
          <div className="text-[11.5px] text-faint num truncate">{x.s}</div>
        </div>
      ))}
    </div>
  );
}

function Legend({ overview, catFilter, setCatFilter, metric }: { overview: Overview | null; catFilter: number | null; setCatFilter: (c: number | null) => void; metric: Metric }) {
  if (!overview) return null;
  const cats = overview.cats.filter((c) => c.alloc > 0 || c.size > 0).sort((a, b) => (metric === "alloc" ? b.alloc - a.alloc : b.size - a.size));
  return (
    <div className="flex flex-wrap gap-x-3 gap-y-1 justify-end max-w-[70%]">
      {cats.map((c) => (
        <button key={c.cat} onClick={() => setCatFilter(catFilter === c.cat ? null : c.cat)} className={cx("flex items-center gap-1.5 text-[11.5px] rounded px-1 hover:bg-surface3", catFilter != null && catFilter !== c.cat ? "opacity-40" : "")} title={`${c.name}: ${num(c.count)} files`}>
          <span className="w-2.5 h-2.5 rounded-sm" style={{ background: CAT_COLORS[c.cat] }} />
          <span className="text-muted">{c.name}</span>
          <span className="num text-ink">{bytes(metric === "alloc" ? c.alloc : c.size)}</span>
        </button>
      ))}
    </div>
  );
}

function ScanPanel({ drives, picked, setPicked, scans, meta, onScan, onFolder, idle, loading, onOpen, onDelete }: { drives: Drive[]; picked: Set<string>; setPicked: (s: Set<string>) => void; scans: ScanMeta[]; meta: ScanMeta | null; onScan: (roots: string[]) => void; onFolder: () => void; idle: boolean; loading: boolean; onOpen: (id: string) => void; onDelete: (id: string) => void }) {
  const toggle = (l: string) => {
    const n = new Set(picked);
    n.has(l) ? n.delete(l) : n.add(l);
    setPicked(n);
  };
  return (
    <Card title="Scan" tip="Scans read directory metadata only (names, sizes, dates); file contents are never opened. Without administrator rights some protected folders cannot be read.">
      <div className="flex flex-wrap gap-3 items-stretch">
        {drives.map((d) => {
          const used = d.total - d.free;
          const on = picked.has(d.letter);
          return (
            <button key={d.letter} onClick={() => toggle(d.letter)} className={cx("text-left rounded-lg border px-3 py-2 w-56 transition-colors", on ? "border-accent bg-accentsoft" : "border-line bg-surface2 hover:border-line2")}>
              <div className="flex items-center justify-between">
                <span className="font-medium flex items-center gap-2">
                  <Icon.drive className="text-muted" /> {d.letter} {d.label && <span className="text-muted font-normal">{d.label}</span>}
                </span>
                <input type="checkbox" readOnly checked={on} />
              </div>
              <Bar value={used} max={d.total} tone="auto" className="mt-2" />
              <div className="text-[11.5px] text-muted num mt-1">
                {bytes(d.free)} free of {bytes(d.total)} · {d.fs}
              </div>
            </button>
          );
        })}
        <div className="flex flex-col gap-2 justify-center">
          <Btn kind="primary" disabled={!idle || picked.size === 0} onClick={() => onScan([...picked].map((l) => l + "\\"))}>
            Scan {picked.size > 1 ? `${picked.size} drives` : "selected drive"}
          </Btn>
          <Btn disabled={!idle} onClick={onFolder}>
            Scan a folder…
          </Btn>
        </div>
      </div>
      {scans.length > 0 && (
        <div className="mt-4 border-t border-line pt-3">
          <div className="text-[11px] uppercase tracking-wide text-muted mb-1.5">Saved scans</div>
          <div className="flex flex-wrap gap-2">
            {scans.slice(0, 8).map((s) => (
              <div key={s.id} className={cx("flex items-center gap-2 rounded-lg border px-2.5 py-1.5", meta?.id === s.id ? "border-accent/60 bg-accentsoft" : "border-line bg-surface2")}>
                <button className="text-left" disabled={loading || !idle} onClick={() => onOpen(s.id)}>
                  <div className="text-[12.5px] font-medium">{s.root}</div>
                  <div className="text-[11px] text-muted num">
                    {bytes(s.totalAlloc)} · {ago(s.startedUnix)}
                  </div>
                </button>
                <button className="text-faint hover:text-bad px-1" title="Delete this saved scan" onClick={() => onDelete(s.id)}>
                  ×
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}

function ScanProgressCard({ p, saving, batch }: { p: import("../lib/types").ScanProgress | null; saving: boolean; batch: { index: number; total: number } | null }) {
  return (
    <Card title={`Scanning ${p?.root ?? ""}${batch && batch.total > 1 ? ` (${batch.index + 1} of ${batch.total})` : ""}`} right={<Btn kind="danger" small onClick={() => api.scanCancel()}>Cancel scan</Btn>}>
      <div className="flex items-center gap-4">
        <Spinner size={22} />
        <div className="grid grid-cols-2 md:grid-cols-5 gap-4 flex-1 min-w-0">
          <div>
            <div className="text-[11px] text-muted">Files</div>
            <div className="text-[18px] font-semibold num">{num(p?.files ?? 0)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted">Folders</div>
            <div className="text-[18px] font-semibold num">{num(p?.dirs ?? 0)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted">Found so far</div>
            <div className="text-[18px] font-semibold num">{bytes(p?.bytes ?? 0)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted">Errors</div>
            <div className={cx("text-[18px] font-semibold num", (p?.errors ?? 0) > 0 && "text-warn")}>{num(p?.errors ?? 0)}</div>
          </div>
          <div>
            <div className="text-[11px] text-muted">Elapsed</div>
            <div className="text-[18px] font-semibold num">{dur((p?.elapsedMs ?? 0) / 1000)}</div>
          </div>
        </div>
      </div>
      <div className="mt-3 text-[11.5px] text-faint truncate num" title={p?.current}>
        {saving ? "Saving scan to disk…" : p?.current || "Starting…"}
      </div>
      <div className="mt-1 text-[11.5px] text-muted">The interface stays fully usable while scanning. Cancelling discards the partial result.</div>
    </Card>
  );
}
