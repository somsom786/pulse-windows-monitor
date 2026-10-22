import { memo, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { MemSnap, ProcRow } from "../lib/types";
import { bytes, fileTimeToMs, num, pct, rate, timeOfDay } from "../lib/format";
import { Badge, Bar, Btn, Card, cx, Empty, Icon, Info, Tabs, useConfirm } from "../components/ui";
import { useMenu } from "../components/menu";
import { SpikeInvestigation } from "./Spikes";

// ---------- icon cache ----------
const iconCache = new Map<string, Promise<string | null>>();
function getIcon(path: string) {
  let p = iconCache.get(path);
  if (!p) {
    p = api.processIcon(path).catch(() => null);
    iconCache.set(path, p);
  }
  return p;
}
function ProcIcon({ path }: { path: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    if (path) getIcon(path).then((s) => live && setSrc(s));
    return () => {
      live = false;
    };
  }, [path]);
  if (src) return <img src={`data:image/png;base64,${src}`} width={16} height={16} className="shrink-0" alt="" />;
  return <span className="w-4 h-4 shrink-0 rounded-[4px] bg-surface3 border border-line" />;
}

// ---------- columns ----------
type ColId = "pws" | "ws" | "commit" | "cpu" | "gpu" | "gpuMem" | "io" | "threads" | "handles" | "started" | "path";
interface Col {
  id: ColId;
  label: string;
  tip?: string;
  get: (r: Agg) => number | string | null;
  fmt: (v: any, r: Agg) => string;
  w: number;
  right?: boolean;
  def: boolean;
}

interface Agg {
  key: string;
  name: string;
  path: string;
  count: number;
  procs: ProcRow[];
  pid: number;
  pws: number;
  ws: number;
  commit: number;
  cpu: number;
  gpu: number | null;
  gpuMem: number | null;
  io: number;
  ioR: number;
  ioW: number;
  threads: number;
  handles: number;
  started: number;
}

const COLS: Col[] = [
  { id: "pws", label: "Private memory", tip: "Private working set: physical RAM used only by this process (not shared with others). This is closest to the RAM you would get back by closing it.", get: (r) => r.pws, fmt: (v) => bytes(v), w: 150, right: true, def: true },
  { id: "ws", label: "Working set", tip: "Physical RAM currently mapped into the process, including shared libraries. Adding working sets across processes counts shared pages more than once, so the sum can exceed real usage.", get: (r) => r.ws, fmt: (v) => bytes(v), w: 100, right: true, def: true },
  { id: "commit", label: "Committed", tip: "Private bytes: virtual memory the process has committed, backed by RAM or the pagefile. Large values here with a small working set mean memory that has been paged out or never touched.", get: (r) => r.commit, fmt: (v) => bytes(v), w: 100, right: true, def: true },
  { id: "cpu", label: "CPU", tip: "Share of total CPU capacity (all logical processors) used since the last sample.", get: (r) => r.cpu, fmt: (v) => (v < 0.05 ? "0%" : v.toFixed(1) + "%"), w: 70, right: true, def: true },
  { id: "gpu", label: "GPU", tip: "Highest single GPU engine utilization for this process (Windows GPU Engine counters).", get: (r) => r.gpu, fmt: (v) => (v == null ? "—" : v < 0.05 ? "0%" : v.toFixed(1) + "%"), w: 70, right: true, def: true },
  { id: "gpuMem", label: "GPU memory", tip: "Dedicated video memory (VRAM) in use by this process, from the Windows GPU Process Memory counters.", get: (r) => r.gpuMem, fmt: (v) => (v == null ? "—" : v === 0 ? "0" : bytes(v)), w: 100, right: true, def: true },
  { id: "io", label: "I/O rate", tip: "Bytes per second read + written through Windows I/O (files, pipes and network combined — it is not disk-only). Hover for the read/write split. Per-process network bytes are not available without elevated tracing.", get: (r) => r.io, fmt: (v) => (v < 1 ? "—" : rate(v)), w: 100, right: true, def: true },
  { id: "threads", label: "Threads", get: (r) => r.threads, fmt: (v) => num(v), w: 70, right: true, def: false },
  { id: "handles", label: "Handles", get: (r) => r.handles, fmt: (v) => num(v), w: 80, right: true, def: false },
  { id: "started", label: "Started", get: (r) => r.started, fmt: (v) => (v ? new Date(v).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "—"), w: 130, def: true },
  { id: "path", label: "Path", get: (r) => r.path, fmt: (v) => v || "—", w: 320, def: false },
];

function aggregate(procs: ProcRow[], group: boolean): Agg[] {
  const mk = (key: string, list: ProcRow[]): Agg => {
    const first = list[0];
    const sum = (f: (p: ProcRow) => number) => list.reduce((a, p) => a + f(p), 0);
    const anyGpu = list.some((p) => p.gpu != null);
    return {
      key,
      name: first.name,
      path: first.path,
      count: list.length,
      procs: list,
      pid: first.pid,
      pws: sum((p) => p.privateWs),
      ws: sum((p) => p.workingSet),
      commit: sum((p) => p.commit),
      cpu: sum((p) => p.cpu),
      gpu: anyGpu ? Math.min(100, sum((p) => p.gpu ?? 0)) : null,
      gpuMem: anyGpu ? sum((p) => p.gpuMem ?? 0) : null,
      io: sum((p) => p.ioReadBps + p.ioWriteBps),
      ioR: sum((p) => p.ioReadBps),
      ioW: sum((p) => p.ioWriteBps),
      threads: sum((p) => p.threads),
      handles: sum((p) => p.handles),
      started: Math.min(...list.map((p) => fileTimeToMs(Number(p.createTime)))),
    };
  };
  if (!group) return procs.map((p) => mk(`${p.pid}:${p.createTime}`, [p]));
  const m = new Map<string, ProcRow[]>();
  for (const p of procs) {
    const k = (p.path || p.name).toLowerCase() + "|" + p.name.toLowerCase();
    const a = m.get(k);
    if (a) a.push(p);
    else m.set(k, [p]);
  }
  return [...m.entries()].map(([k, l]) => mk(k, l));
}

export function Processes() {
  const { snap, procs, viewOpts } = useStore();
  const [tab, setTab] = useState<string>(viewOpts.tab ?? "live");
  const m = snap?.mem;
  return (
    <div className="h-full overflow-auto p-5 space-y-4">
      <div>
        <h1 className="text-[20px] font-semibold tracking-tight">Processes & Memory</h1>
        <p className="text-muted text-[12.5px] mt-0.5">Find what is using your RAM now, and what used it when it spiked.</p>
      </div>
      {m ? <MemoryComposition m={m} procs={procs} /> : <Card><div className="h-28 skeleton" /></Card>}
      <Card pad={false}>
        <div className="px-4">
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { id: "live", label: "Live processes", badge: <Badge>{procs.length}</Badge> },
              { id: "spikes", label: "Spike investigation" },
            ]}
          />
        </div>
        {tab === "live" ? <Live /> : <SpikeInvestigation />}
      </Card>
    </div>
  );
}

// ---------- memory composition ----------
function MemoryComposition({ m, procs }: { m: MemSnap; procs: ProcRow[] }) {
  const gb = (v: number | null) => (v == null ? "—" : bytes(v));
  const parts = [
    { id: "use", label: "In use", v: m.inUse, c: "#a78bfa", tip: "Memory actively held by processes, drivers and the Windows kernel. This is memory that cannot be reclaimed without closing something." },
    { id: "mod", label: "Modified", v: m.modified, c: "#f59e0b", tip: "Pages changed in RAM that Windows must write to disk before reusing. Normally small." },
    { id: "std", label: "Standby (cache)", v: m.standby, c: "#38bdf8", tip: "Cached file data and recently used pages. Windows can reuse it instantly on demand, so a large standby list is healthy and counts toward Available — it is not 'wasted' or a leak." },
    { id: "free", label: "Free", v: m.free, c: "#3b4558", tip: "Memory holding nothing at all. Windows deliberately keeps this small and fills the rest with useful cache." },
  ];
  const sumProc = procs.filter((p) => p.pid !== 0 && p.name !== "Memory Compression").reduce((a, p) => a + p.privateWs, 0);
  const commitPct = (m.commitTotal * 100) / m.commitLimit;
  const pressTone = m.pressure === "normal" ? "good" : m.pressure === "elevated" ? "warn" : "bad";
  return (
    <Card
      title="Memory composition"
      tip="How Windows is using physical RAM right now. The segments add up to total installed memory."
      right={
        <span className="flex items-center gap-2 text-[12px]">
          <span className="text-muted">Memory pressure</span>
          <Badge tone={pressTone as any}>{m.pressure}</Badge>
          <Info text="Pressure is judged from two facts: how much memory is still Available (free + reusable cache) and how much of the commit limit is promised. Cached/standby memory alone is not pressure. Real trouble looks like low Available memory together with commit close to its limit and heavy paging." />
        </span>
      }
    >
      <div className="flex h-7 rounded-lg overflow-hidden bg-surface3 border border-line">
        {parts.map((p) => (
          <div key={p.id} className="relative h-full transition-[width] duration-500" style={{ width: `${((p.v ?? 0) * 100) / m.total}%`, background: p.c }} title={`${p.label}: ${gb(p.v)}`} />
        ))}
      </div>
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-x-6 gap-y-2 mt-3">
        {parts.map((p) => (
          <div key={p.id}>
            <div className="flex items-center gap-1.5 text-[11.5px] text-muted">
              <span className="w-2.5 h-2.5 rounded-sm" style={{ background: p.c }} />
              {p.label}
              <Info text={p.tip} />
            </div>
            <div className="text-[17px] font-semibold num">{p.v == null ? "Unavailable" : bytes(p.v)}</div>
            <div className="text-[11px] text-faint num">{p.v == null ? "counter not exposed" : pct((p.v * 100) / m.total, 1) + " of RAM"}</div>
          </div>
        ))}
      </div>
      <div className="grid md:grid-cols-3 gap-4 mt-4 pt-3 border-t border-line">
        <div>
          <div className="text-[11.5px] text-muted flex items-center gap-1">
            Available <Info text="Memory that can be given to a program immediately: free + standby. This is the number that matters when asking 'am I running out?'" />
          </div>
          <div className="text-[15px] font-semibold num">
            {bytes(m.available)} <span className="text-muted font-normal text-[12px]">of {bytes(m.total)}</span>
          </div>
          <Bar value={m.total - m.available} max={m.total} tone="auto" className="mt-1.5" />
          <div className="text-[11px] text-faint mt-1">{m.pressureReason}</div>
        </div>
        <div>
          <div className="text-[11.5px] text-muted flex items-center gap-1">
            Committed <Info text="Virtual memory promised to programs (RAM + pagefile backing). If committed reaches the commit limit, Windows cannot give programs more memory even if RAM looks free — apps then fail to allocate." />
          </div>
          <div className="text-[15px] font-semibold num">
            {bytes(m.commitTotal)} <span className="text-muted font-normal text-[12px]">of {bytes(m.commitLimit)} limit</span>
          </div>
          <Bar value={m.commitTotal} max={m.commitLimit} tone="auto" className="mt-1.5" />
          <div className="text-[11px] text-faint mt-1 num">
            Peak {bytes(m.commitPeak)} · {commitPct.toFixed(0)}% of limit · Pagefile {m.pagefiles.length ? `${bytes(m.pagefiles.reduce((a, p) => a + p.inUse, 0))} used of ${bytes(m.pagefiles.reduce((a, p) => a + p.total, 0))}` : "none"}
          </div>
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-1 content-start">
          <KV l="Kernel paged pool" v={bytes(m.kernelPaged)} tip="Kernel memory that can be written to the pagefile. Growth here is usually a driver." />
          <KV l="Kernel non-paged pool" v={bytes(m.kernelNonpaged)} tip="Kernel memory that must always stay in RAM." />
          <KV l="Compressed" v={m.compressed != null ? bytes(m.compressed) : "—"} tip="Working set of the 'Memory Compression' system process, an estimate of the RAM footprint of the compression store (not the uncompressed size, and not a saving figure). Already included in 'In use'." />
          <KV l="Paging in/out" v={m.pagesInPerSec != null ? `${m.pagesInPerSec.toFixed(0)} / ${(m.pagesOutPerSec ?? 0).toFixed(0)} /s` : "—"} tip="Pages read from / written to the pagefile and mapped files per second. Sustained high 'out' with low Available memory is a true sign of memory pressure." />
        </div>
      </div>
      <div className="mt-3 rounded-lg bg-surface2 border border-line px-3 py-2 text-[12px] text-muted">
        Per-process <b className="text-ink">private memory</b> adds up to <b className="text-ink num">{bytes(sumProc)}</b>, versus <b className="text-ink num">{bytes(m.inUse)}</b> in use system-wide. The difference is the kernel and drivers ({bytes(m.kernelPaged + m.kernelNonpaged)} pools), compressed memory, and shared or mapped memory that no single process owns. The totals are not expected to match.
      </div>
    </Card>
  );
}

function KV({ l, v, tip }: { l: string; v: string; tip: string }) {
  return (
    <div>
      <div className="text-[11px] text-muted flex items-center gap-1">
        {l} <Info text={tip} />
      </div>
      <div className="text-[13px] font-medium num">{v}</div>
    </div>
  );
}

// ---------- live table ----------
function Live() {
  const procs = useStore((s) => s.procs);
  const toast = useStore((s) => s.toast);
  const [q, setQ] = useState("");
  const [group, setGroup] = useState(true);
  const [sortKey, setSortKey] = useState<ColId | "name">("pws");
  const [desc, setDesc] = useState(true);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [cols, setCols] = useState<Set<ColId>>(() => {
    try {
      const s = localStorage.getItem("pulse.cols");
      if (s) return new Set(JSON.parse(s));
    } catch {}
    return new Set(COLS.filter((c) => c.def).map((c) => c.id));
  });
  const [colMenu, setColMenu] = useState(false);
  const menu = useMenu();
  const { ask, node } = useConfirm();
  const frozen = useRef(false);
  const [freeze, setFreeze] = useState(false);
  const [shown, setShown] = useState<ProcRow[]>(procs);
  frozen.current = freeze;
  useEffect(() => {
    if (!frozen.current) setShown(procs);
  }, [procs]);

  useEffect(() => {
    try {
      localStorage.setItem("pulse.cols", JSON.stringify([...cols]));
    } catch {}
  }, [cols]);

  const visibleCols = COLS.filter((c) => cols.has(c.id));
  const filtered = useMemo(() => {
    const t = q.trim().toLowerCase();
    const base = shown.filter((p) => p.pid !== 0);
    if (!t) return base;
    return base.filter((p) => p.name.toLowerCase().includes(t) || p.path.toLowerCase().includes(t) || String(p.pid) === t);
  }, [shown, q]);
  const aggs = useMemo(() => {
    const a = aggregate(filtered, group);
    const col = COLS.find((c) => c.id === sortKey);
    const dir = desc ? -1 : 1;
    a.sort((x, y) => {
      if (sortKey === "name") return dir * x.name.localeCompare(y.name);
      const vx = col!.get(x),
        vy = col!.get(y);
      if (vx == null && vy == null) return 0;
      if (vx == null) return 1;
      if (vy == null) return -1;
      if (typeof vx === "string") return dir * vx.localeCompare(vy as string);
      return dir * (vx - (vy as number));
    });
    return a;
  }, [filtered, group, sortKey, desc]);
  const maxPws = aggs.reduce((m, a) => Math.max(m, a.pws), 1);
  const totals = useMemo(() => ({ pws: filtered.reduce((a, p) => a + p.privateWs, 0), count: filtered.length }), [filtered]);

  const sortBy = (k: ColId | "name") => {
    if (sortKey === k) setDesc(!desc);
    else {
      setSortKey(k);
      setDesc(k !== "name" && k !== "path");
    }
  };

  const endProcs = async (list: ProcRow[], label: string) => {
    const ok = await ask(
      `End ${label}?`,
      <div className="space-y-2">
        <div>Ending a process closes it immediately. Unsaved work in it will be lost.</div>
        <div className="rounded-md bg-surface2 border border-line p-2 text-[12px] max-h-32 overflow-auto text-ink">{list.map((p) => `${p.name} (PID ${p.pid})`).join("\n")}</div>
        <div className="text-faint text-[12px]">Critical Windows processes are blocked and cannot be ended from PULSE.</div>
      </div>,
      "End process",
      true,
    );
    if (!ok) return;
    let done = 0;
    for (const p of list) {
      try {
        await api.terminate(p.pid, p.createTime, p.name);
        done++;
      } catch (e) {
        toast({ kind: "bad", title: `Could not end ${p.name} (${p.pid})`, body: String(e) });
      }
    }
    if (done) toast({ kind: "good", title: `Ended ${done} process${done > 1 ? "es" : ""}` });
  };

  const rowMenu = (e: React.MouseEvent, procsList: ProcRow[], name: string) => {
    const p = procsList[0];
    const multi = procsList.length > 1;
    menu.open(e, [
      { label: "Show file location", disabled: !p.path, onClick: () => api.reveal(p.path).catch((er) => toast({ kind: "bad", title: "Failed", body: String(er) })) },
      { label: "Copy path", disabled: !p.path, onClick: () => navigator.clipboard.writeText(p.path) },
      { label: "Properties…", disabled: !p.path, onClick: () => api.props(p.path) },
      { label: "", sep: true },
      { label: multi ? `End all ${procsList.length} processes…` : "End process…", danger: true, onClick: () => endProcs(procsList, multi ? `all ${procsList.length} ${name} processes` : `${name} (PID ${p.pid})`) },
    ]);
  };

  const Header = ({ id, label, tip, right, w }: { id: ColId | "name"; label: string; tip?: string; right?: boolean; w?: number }) => (
    <th style={{ width: w }} className={cx("sticky top-0 z-[1] bg-surface px-3 py-2 text-[11px] font-medium uppercase tracking-wide text-muted border-b border-line whitespace-nowrap cursor-pointer hover:text-ink select-none", right ? "text-right" : "text-left")} onClick={() => sortBy(id)}>
      <span className="inline-flex items-center gap-1">
        {label}
        {tip && <Info text={tip} />}
        {sortKey === id && <span className="text-accent">{desc ? "↓" : "↑"}</span>}
      </span>
    </th>
  );

  const renderCells = (a: Agg, child = false) =>
    visibleCols.map((c) => {
      const v = c.get(a);
      let txt = v == null && c.id !== "gpu" && c.id !== "gpuMem" ? "—" : c.fmt(v, a);
      return (
        <td key={c.id} className={cx("px-3 py-1.5 num whitespace-nowrap", c.right && "text-right", c.id === "path" && "text-faint truncate max-w-[320px]", c.id === "started" && "text-muted", child && "text-muted")} title={c.id === "io" ? `Read ${rate(a.ioR)} · Write ${rate(a.ioW)}` : c.id === "path" ? a.path : undefined}>
          {c.id === "pws" ? (
            <div className="flex items-center justify-end gap-2">
              <div className="w-14 h-1.5 rounded-full bg-surface3 overflow-hidden">
                <div className="h-full rounded-full" style={{ width: `${Math.max(0.5, (a.pws / maxPws) * 100)}%`, background: "#a78bfa" }} />
              </div>
              <span className="w-16 text-right">{txt}</span>
            </div>
          ) : (
            txt
          )}
        </td>
      );
    });

  return (
    <div>
      {node}
      {menu.node}
      <div className="flex items-center gap-3 px-4 py-3 border-b border-line flex-wrap">
        <div className="relative">
          <Icon.search className="absolute left-2.5 top-1/2 -translate-y-1/2 text-faint" />
          <input type="text" placeholder="Search name, path or PID…" className="pl-8 w-64" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <label className="flex items-center gap-2 text-[12.5px] text-muted cursor-pointer">
          <input type="checkbox" checked={group} onChange={(e) => setGroup(e.target.checked)} />
          Group by application
          <Info text="Combines all processes that run the same program (for example the many chrome.exe processes) into one expandable row with summed totals." />
        </label>
        <label className="flex items-center gap-2 text-[12.5px] text-muted cursor-pointer">
          <input type="checkbox" checked={freeze} onChange={(e) => setFreeze(e.target.checked)} />
          Freeze view
          <Info text="Stops the table from updating so you can read or sort it without rows jumping around." />
        </label>
        <span className="flex-1" />
        <span className="text-[12px] text-muted num">
          {num(totals.count)} processes · {bytes(totals.pws)} private memory
        </span>
        <div className="relative">
          <Btn small onClick={() => setColMenu(!colMenu)}>
            Columns
          </Btn>
          {colMenu && (
            <>
              <div className="fixed inset-0 z-[60]" onClick={() => setColMenu(false)} />
              <div className="absolute right-0 top-9 z-[61] w-56 rounded-lg border border-line2 bg-surface3 p-2 shadow-2xl">
                {COLS.map((c) => (
                  <label key={c.id} className="flex items-center gap-2 px-2 py-1 rounded hover:bg-line cursor-pointer text-[12.5px]">
                    <input
                      type="checkbox"
                      checked={cols.has(c.id)}
                      onChange={(e) => {
                        const n = new Set(cols);
                        e.target.checked ? n.add(c.id) : n.delete(c.id);
                        setCols(n);
                      }}
                    />
                    {c.label}
                  </label>
                ))}
                <div className="text-[11px] text-faint px-2 pt-1 border-t border-line mt-1">Per-process network usage is not available without elevated event tracing, so it is not shown.</div>
              </div>
            </>
          )}
        </div>
      </div>
      {procs.length === 0 ? (
        <div className="py-12 flex justify-center"><div className="skeleton w-64 h-4" /></div>
      ) : (
        <div className="overflow-auto" style={{ maxHeight: "calc(100vh - 420px)", minHeight: 320 }}>
          <table className="w-full border-collapse text-[12.5px]">
            <thead>
              <tr>
                <Header id="name" label="Name" />
                <th className="sticky top-0 z-[1] bg-surface px-3 py-2 text-[11px] font-medium uppercase tracking-wide text-muted border-b border-line text-right w-[70px]">PID</th>
                {visibleCols.map((c) => (
                  <Header key={c.id} id={c.id} label={c.label} tip={c.tip} right={c.right} w={c.w} />
                ))}
                <th className="sticky top-0 bg-surface border-b border-line w-8" />
              </tr>
            </thead>
            <tbody>
              {aggs.map((a) => {
                const isOpen = open.has(a.key);
                const multi = a.count > 1;
                return (
                  <Fragment key={a.key}>
                    <tr className="border-b border-line/50 hover:bg-surface2 group" onContextMenu={(e) => rowMenu(e, a.procs, a.name)}>
                      <td className="px-3 py-1.5">
                        <div className="flex items-center gap-2 min-w-0">
                          {multi ? (
                            <button className="w-4 text-muted hover:text-ink" onClick={() => setOpen((s) => { const n = new Set(s); isOpen ? n.delete(a.key) : n.add(a.key); return n; })}>
                              <Icon.chevron className={cx("transition-transform", isOpen && "rotate-90")} />
                            </button>
                          ) : (
                            <span className="w-4" />
                          )}
                          <ProcIcon path={a.path} />
                          <span className="truncate" title={a.path || a.name}>
                            {a.name}
                          </span>
                          {multi && <Badge>{a.count}</Badge>}
                        </div>
                      </td>
                      <td className="px-3 py-1.5 text-right num text-faint">{multi ? "" : a.pid}</td>
                      {renderCells(a)}
                      <td className="px-1 text-right">
                        <button className="opacity-0 group-hover:opacity-100 text-muted hover:text-ink p-1 rounded hover:bg-line" onClick={(e) => rowMenu(e, a.procs, a.name)}>
                          <Icon.more />
                        </button>
                      </td>
                    </tr>
                    {multi &&
                      isOpen &&
                      [...a.procs]
                        .sort((x, y) => y.privateWs - x.privateWs)
                        .map((p) => {
                          const c = aggregate([p], false)[0];
                          return (
                            <tr key={p.pid + ":" + p.createTime} className="border-b border-line/30 bg-surface2/40 hover:bg-surface2 group" onContextMenu={(e) => rowMenu(e, [p], p.name)}>
                              <td className="px-3 py-1 pl-14 text-muted truncate">{p.name}</td>
                              <td className="px-3 py-1 text-right num text-faint">{p.pid}</td>
                              {renderCells(c, true)}
                              <td className="px-1 text-right">
                                <button className="opacity-0 group-hover:opacity-100 text-muted hover:text-ink p-1 rounded hover:bg-line" onClick={(e) => rowMenu(e, [p], p.name)}>
                                  <Icon.more />
                                </button>
                              </td>
                            </tr>
                          );
                        })}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
          {aggs.length === 0 && <Empty title="No matching processes" />}
        </div>
      )}
    </div>
  );
}

import { Fragment } from "react";
