import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { GrowthRow, PeakRow, Point, SnapAt, SpikeRow, StoreInfo } from "../lib/types";
import { bytes, dateTime, num, pct, timeOfDay } from "../lib/format";
import { Badge, Bar, Btn, cx, Empty, Info, Segmented, Spinner, useInterval } from "../components/ui";
import { baseOption, Chart, line, SERIES, theme } from "../components/Chart";

export const RANGES = [
  { value: "15m", label: "15 min", ms: 15 * 60_000 },
  { value: "1h", label: "1 hour", ms: 3_600_000 },
  { value: "6h", label: "6 hours", ms: 6 * 3_600_000 },
  { value: "24h", label: "24 hours", ms: 24 * 3_600_000 },
  { value: "7d", label: "7 days", ms: 7 * 86_400_000 },
] as const;

export function nearest<T extends { t: number }>(pts: T[], ts: number): T | null {
  let best: T | null = null;
  let bd = Infinity;
  for (const p of pts) {
    const d = Math.abs(p.t - ts);
    if (d < bd) {
      bd = d;
      best = p;
    }
  }
  return best;
}

export function SpikeInvestigation() {
  const settings = useStore((s) => s.settings);
  const navigate = useStore((s) => s.navigate);
  const [range, setRange] = useState<(typeof RANGES)[number]["value"]>("1h");
  const [pts, setPts] = useState<Point[]>([]);
  const [spikes, setSpikes] = useState<SpikeRow[]>([]);
  const [peaks, setPeaks] = useState<PeakRow[]>([]);
  const [growth, setGrowth] = useState<GrowthRow[]>([]);
  const [info, setInfo] = useState<StoreInfo | null>(null);
  const [sel, setSel] = useState<number | null>(null);
  const [snap, setSnap] = useState<SnapAt | null | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  const [appHist, setAppHist] = useState<{ name: string; data: [number, number][] } | null>(null);

  const ms = RANGES.find((r) => r.value === range)!.ms;
  const load = useCallback(async () => {
    const to = Date.now();
    const from = to - ms;
    const [p, s, pk, g, i] = await Promise.all([api.historySeries(from, to, 600), api.historySpikes(from, to, 100), api.historyPeaks(from, to, 20), api.historyGrowth(60), api.storeInfo()]);
    setPts(p);
    setSpikes(s);
    setPeaks(pk);
    setGrowth(g);
    setInfo(i);
    setLoading(false);
  }, [ms]);
  useEffect(() => {
    setLoading(true);
    load();
  }, [load]);
  useInterval(load, 15000, [load]);

  const inspect = useCallback(async (ts: number) => {
    setSel(ts);
    setSnap(undefined);
    setSnap(await api.historySnapAt(ts));
  }, []);

  const total = useStore((s) => s.snap?.mem.total) ?? 0;
  const th = theme();
  const option = useMemo(() => {
    const o: any = baseOption(th, { yFormatter: (v: number) => bytes(v, 0), yMax: total || undefined, legend: true, gridLeft: 56 });
    o.tooltip.valueFormatter = (v: number) => (v == null ? "—" : bytes(v));
    o.series = [
      line("RAM in use", pts.map((p) => [p.t, p.ramUsed]), SERIES.ram, { area: true }),
      line("RAM peak in interval", pts.map((p) => [p.t, p.ramUsedMax]), SERIES.ram, { lineStyle: { width: 0.8, type: "dotted", color: SERIES.ram } }),
      line("Available (lowest)", pts.map((p) => [p.t, p.ramAvailMin]), SERIES.cpu, { lineStyle: { width: 1.2, color: SERIES.cpu } }),
      line("Committed", pts.map((p) => [p.t, p.commit]), SERIES.commit, { lineStyle: { width: 1, type: "dashed", color: SERIES.commit } }),
      {
        name: "Spikes",
        type: "scatter",
        symbolSize: 11,
        itemStyle: { color: th.bad, borderColor: "#fff", borderWidth: 1 },
        data: spikes.filter((s) => s.kind === "memory").map((s) => [s.ts, nearest(pts, s.ts)?.ramUsed ?? s.value]),
        tooltip: { formatter: (p: any) => `Memory spike · ${timeOfDay(p.value[0], true)}` },
        z: 5,
      },
      sel != null
        ? { name: "Selected", type: "line", data: [], markLine: { silent: true, symbol: "none", lineStyle: { color: th.accent, width: 1.5 }, label: { formatter: timeOfDay(sel, true), color: th.accent }, data: [{ xAxis: sel }] } }
        : { name: "Selected", type: "line", data: [] },
    ];
    o.xAxis.min = Date.now() - ms;
    o.xAxis.max = Date.now();
    return o;
  }, [pts, spikes, sel, ms, total, th.accent]);

  const recStart = info?.oldestTs ?? null;
  const covered = recStart != null && recStart > Date.now() - ms ? recStart : null;
  const selPt = sel != null ? nearest(pts, sel) : null;

  return (
    <div className="p-4 space-y-5">
      <div className="flex items-center gap-3 flex-wrap">
        <Segmented value={range} onChange={setRange} options={RANGES.map((r) => ({ value: r.value, label: r.label }))} />
        <span className="text-[12px] text-muted">
          {settings && !settings.recordEnabled ? (
            <Badge tone="warn">Recording is off — enable it in Settings to capture history</Badge>
          ) : info?.oldestTs ? (
            <>
              Recording since <b className="text-ink">{dateTime(info.oldestTs)}</b>. PULSE has no data from before that.
            </>
          ) : (
            "No history recorded yet — it starts filling in as PULSE runs."
          )}
        </span>
        <span className="flex-1" />
        <span className="text-[12px] text-faint">Click the chart to see which processes were using memory at that moment.</span>
      </div>

      <div className="rounded-lg border border-line bg-surface2/40 p-2 relative">
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center z-10 bg-surface/50">
            <Spinner />
          </div>
        )}
        <Chart
          option={option}
          height={240}
          onZr={(e, chart) => {
            const v = chart.convertFromPixel({ gridIndex: 0 }, [e.offsetX, e.offsetY]) as number[];
            if (v && isFinite(v[0])) {
              const p = nearest(pts, v[0]);
              inspect(p ? p.t : v[0]);
            }
          }}
        />
        {covered && <div className="text-[11.5px] text-warn px-2 pb-1">Data before {dateTime(covered)} does not exist — monitoring was not running.</div>}
        {!loading && pts.length === 0 && <div className="text-center text-muted text-[12.5px] pb-4">No samples in this window.</div>}
      </div>

      {sel != null && (
        <div className="rounded-xl border border-accent/30 bg-accentsoft/50 p-4 fade-in">
          <div className="flex items-start justify-between gap-4 flex-wrap">
            <div>
              <div className="text-[11px] uppercase tracking-wide text-accent">Moment inspector</div>
              <div className="text-[15px] font-semibold">{dateTime(sel)}</div>
              {selPt && (
                <div className="text-[12.5px] text-muted num mt-0.5">
                  RAM in use {bytes(selPt.ramUsed)} ({total ? pct(((selPt.ramUsed ?? 0) * 100) / total) : "—"}) · lowest available {bytes(selPt.ramAvailMin)} · CPU {pct(selPt.cpu)} · GPU {selPt.gpuUtil != null ? pct(selPt.gpuUtil) : "—"} · VRAM {selPt.vramUsed != null ? bytes(selPt.vramUsed) : "—"}
                </div>
              )}
            </div>
            <Btn small kind="ghost" onClick={() => setSel(null)}>
              Close
            </Btn>
          </div>
          {snap === undefined ? (
            <div className="py-6 flex justify-center"><Spinner /></div>
          ) : snap === null ? (
            <div className="mt-3 text-[12.5px] text-muted">No process snapshot was recorded near this time (snapshots are taken every {settings?.procSnapshotIntervalS ?? 15}s while monitoring and recording are on).</div>
          ) : (
            <div className="mt-3">
              <div className="text-[12px] text-muted mb-1.5">
                Process snapshot from <b className="text-ink">{timeOfDay(snap.ts, true)}</b>
                {Math.abs(snap.ts - sel) > 2000 ? ` (${Math.round(Math.abs(snap.ts - sel) / 1000)}s ${snap.ts < sel ? "before" : "after"} the selected point)` : ""} — top memory users by private memory:
              </div>
              <div className="text-[11px] text-faint -mt-1 mb-1.5">Snapshots keep roughly the 20 largest processes plus the top CPU and GPU users, so application totals are lower bounds. Private memory does not include kernel, cache or shared memory.</div>
              <AppsTable snap={snap} />
            </div>
          )}
        </div>
      )}

      <div className="grid xl:grid-cols-2 gap-5">
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <h4 className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted flex items-center gap-1.5">
              Detected spikes <Info text="A memory spike is logged when RAM use rises by the configured amount (default 4 GB) within 2 minutes. CPU and GPU spikes are logged when utilization stays at 90–95%+ for 15 seconds. Each spike stores which applications grew the most." />
            </h4>
            <Badge>{spikes.length}</Badge>
          </div>
          {spikes.length === 0 ? (
            <div className="text-[12.5px] text-muted rounded-lg border border-line px-3 py-4">No spikes detected in this window.</div>
          ) : (
            <div className="rounded-lg border border-line divide-y divide-line/70 max-h-[360px] overflow-auto">
              {spikes.map((s) => (
                <div key={s.id} className={cx("px-3 py-2 hover:bg-surface2", sel === s.ts && "bg-accentsoft/50")}>
                  <div className="flex items-center gap-2">
                    <Badge tone={s.kind === "memory" ? "accent" : s.kind === "cpu" ? "warn" : "neutral"}>{s.kind.toUpperCase()}</Badge>
                    <span className="text-[12px] text-muted num">{dateTime(s.ts)}</span>
                    <span className="flex-1" />
                    <Btn small kind="ghost" onClick={() => inspect(s.ts)}>
                      Inspect
                    </Btn>
                  </div>
                  <div className="text-[12.5px] mt-1">{s.summary}</div>
                  {s.detail?.topGrowth?.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1.5">
                      {s.detail.topGrowth.slice(0, 5).map((g: any) => (
                        <span key={g.name} className="rounded-md bg-surface3 px-1.5 py-0.5 text-[11.5px] num" title={g.newProcesses ? `${g.newProcesses} process(es) started after the baseline` : undefined}>
                          {g.name} <b className="text-warn">+{bytes(g.delta)}</b>
                          {g.newProcesses > 0 && <span className="text-faint"> · {g.newProcesses} new</span>}
                        </span>
                      ))}
                      {s.detail.hasBaseline && s.detail.unexplainedBytes > 0 && (
                        <span className="rounded-md border border-line px-1.5 py-0.5 text-[11.5px] num text-muted" title="The part of the system-wide rise that is not visible as process private memory: kernel, file cache, shared memory, or processes too small to be tracked.">
                          not in process memory: {bytes(s.detail.unexplainedBytes)}
                        </span>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>

        <div>
          <div className="flex items-center justify-between mb-1.5">
            <h4 className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted flex items-center gap-1.5">
              Peak memory by application <Info text="Highest combined private memory each program reached in recorded snapshots during this window, with all its processes summed. Click a row to see its memory over time." />
            </h4>
          </div>
          {peaks.length === 0 ? (
            <div className="text-[12.5px] text-muted rounded-lg border border-line px-3 py-4">No process snapshots in this window yet.</div>
          ) : (
            <div className="rounded-lg border border-line max-h-[360px] overflow-auto">
              <table className="w-full text-[12.5px]">
                <tbody>
                  {peaks.map((p) => (
                    <tr key={p.name} className="border-b border-line/60 hover:bg-surface2 cursor-pointer" onClick={async () => setAppHist({ name: p.name, data: (await api.historyApp(p.name, Date.now() - ms, Date.now())) as [number, number][] })}>
                      <td className="px-3 py-1.5">
                        {p.name} {p.maxInstances > 1 && <Badge>{p.maxInstances}</Badge>}
                      </td>
                      <td className="px-3 py-1.5 text-right num font-medium">{bytes(p.peakPrivateWs)}</td>
                      <td className="px-3 py-1.5 text-right num text-muted">{timeOfDay(p.peakTs)}</td>
                      <td className="px-2 text-right">
                        <button className="text-accent text-[11.5px] hover:underline" onClick={(e) => { e.stopPropagation(); inspect(p.peakTs); }}>
                          inspect
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {appHist && (
            <div className="mt-3 rounded-lg border border-line p-2">
              <div className="flex justify-between px-1 text-[12px]">
                <b>{appHist.name}</b> <button className="text-muted hover:text-ink" onClick={() => setAppHist(null)}>close</button>
              </div>
              <Chart height={140} option={(() => { const o: any = baseOption(th, { yFormatter: (v: number) => bytes(v, 0), gridLeft: 56 }); o.tooltip.valueFormatter = (v: number) => bytes(v); o.series = [line(appHist.name, appHist.data, SERIES.ram, { area: true })]; return o; })()} />
            </div>
          )}
        </div>
      </div>

      <div>
        <h4 className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted mb-1.5 flex items-center gap-1.5">
          Sustained growth observations (last hour) <Info text="Processes whose private memory rose steadily over at least 15 minutes without large drops. This can be a leak, but also a cache, an open document or a game loading assets — treat it as a lead, not a verdict." />
        </h4>
        {growth.length === 0 ? (
          <div className="text-[12.5px] text-muted rounded-lg border border-line px-3 py-3">No process has shown sustained growth in the recorded data.</div>
        ) : (
          <div className="rounded-lg border border-line divide-y divide-line/70">
            {growth.map((g) => (
              <div key={g.pid} className="px-3 py-2 flex items-center gap-4">
                <div className="min-w-0 flex-1">
                  <div className="font-medium">{g.name} <span className="text-faint text-[11.5px]">PID {g.pid}</span></div>
                  <div className="text-[12px] text-muted">Sustained memory growth observed: {bytes(g.startWs)} → {bytes(g.nowWs)} over {g.minutes.toFixed(0)} min</div>
                </div>
                <div className="text-warn font-semibold num">+{bytes(g.growthPerHour)}/h</div>
              </div>
            ))}
          </div>
        )}
      </div>
      <div className="text-[12px] text-muted">
        Want a notification when memory gets tight? <button className="text-accent hover:underline" onClick={() => navigate("settings")}>Configure memory-pressure alerts</button>.
      </div>
    </div>
  );
}

export function AppsTable({ snap }: { snap: SnapAt }) {
  const max = snap.apps[0]?.privateWs || 1;
  return (
    <div className="rounded-lg border border-line bg-surface overflow-hidden">
      <table className="w-full text-[12.5px]">
        <thead>
          <tr className="text-muted text-[11px] uppercase tracking-wide text-left">
            <th className="px-3 py-1.5">Application</th>
            <th className="px-3 py-1.5 text-right">Private memory</th>
            <th className="px-3 py-1.5 text-right">CPU</th>
            <th className="px-3 py-1.5 text-right">GPU mem</th>
          </tr>
        </thead>
        <tbody>
          {snap.apps.slice(0, 15).map((a) => (
            <tr key={a.name} className="border-t border-line/60">
              <td className="px-3 py-1.5">
                {a.name} {a.count > 1 && <Badge>{a.count}</Badge>}
              </td>
              <td className="px-3 py-1.5">
                <div className="flex items-center justify-end gap-2">
                  <div className="w-28 h-1.5 rounded-full bg-surface3 overflow-hidden">
                    <div className="h-full rounded-full bg-[#a78bfa]" style={{ width: `${(a.privateWs / max) * 100}%` }} />
                  </div>
                  <span className="num w-16 text-right">{bytes(a.privateWs)}</span>
                </div>
              </td>
              <td className="px-3 py-1.5 text-right num text-muted">{a.cpu >= 0.1 ? a.cpu.toFixed(1) + "%" : "0%"}</td>
              <td className="px-3 py-1.5 text-right num text-muted">{a.gpuMem != null && a.gpuMem > 0 ? bytes(a.gpuMem) : "—"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
