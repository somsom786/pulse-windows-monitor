import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { AlertRow, Point, SnapAt, SpikeRow, StoreInfo } from "../lib/types";
import { bytes, dateTime, num, pct, rate, temp, timeOfDay } from "../lib/format";
import { Badge, Btn, Card, cx, Info, Segmented, Spinner, useInterval } from "../components/ui";
import { baseOption, Chart, line, SERIES, theme } from "../components/Chart";
import { AppsTable, nearest } from "./Spikes";

const RANGES = [
  { value: "15m", label: "15 min", ms: 15 * 60_000 },
  { value: "1h", label: "1 hour", ms: 3_600_000 },
  { value: "6h", label: "6 hours", ms: 6 * 3_600_000 },
  { value: "24h", label: "24 hours", ms: 24 * 3_600_000 },
  { value: "7d", label: "7 days", ms: 7 * 86_400_000 },
  { value: "30d", label: "30 days", ms: 30 * 86_400_000 },
] as const;

type PanelId = "util" | "mem" | "temp" | "io";

export function History() {
  const settings = useStore((s) => s.settings);
  const toast = useStore((s) => s.toast);
  const unit = settings?.tempUnit ?? "C";
  const [range, setRange] = useState<(typeof RANGES)[number]["value"]>("1h");
  const [pts, setPts] = useState<Point[]>([]);
  const [spikes, setSpikes] = useState<SpikeRow[]>([]);
  const [alerts, setAlerts] = useState<AlertRow[]>([]);
  const [info, setInfo] = useState<StoreInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [sel, setSel] = useState<number | null>(null);
  const [snap, setSnap] = useState<SnapAt | null | undefined>(undefined);
  const [panels, setPanels] = useState<Record<PanelId, boolean>>({ util: true, mem: true, temp: true, io: true });
  const [series, setSeries] = useState({ cpu: true, ram: true, gpu: true, vram: true });
  const ms = RANGES.find((r) => r.value === range)!.ms;
  const total = useStore((s) => s.snap?.mem.total) ?? 1;
  const vtotal = useStore((s) => s.snap?.gpu?.vramTotal.value) ?? null;

  const load = useCallback(async () => {
    const to = Date.now();
    const from = to - ms;
    const [p, sp, al, i] = await Promise.all([api.historySeries(from, to, 700), api.historySpikes(from, to, 200), api.historyAlerts(), api.storeInfo()]);
    setPts(p);
    setSpikes(sp);
    setAlerts(al);
    setInfo(i);
    setLoading(false);
  }, [ms]);
  useEffect(() => {
    setLoading(true);
    load();
  }, [load]);
  useInterval(load, 20000, [load]);

  const inspect = async (ts: number) => {
    setSel(ts);
    setSnap(undefined);
    setSnap(await api.historySnapAt(ts));
  };

  const th = theme();
  const mk = (extra: any, yFmt: (v: number) => string, valFmt: (v: number) => string, ymax?: number | "dataMax", left = 52) => {
    const o: any = baseOption(th, { yFormatter: yFmt, yMax: ymax, legend: true, gridLeft: left });
    o.tooltip.valueFormatter = (v: number) => (v == null ? "—" : valFmt(v));
    o.xAxis.min = Date.now() - ms;
    o.xAxis.max = Date.now();
    o.axisPointer = { link: [{ xAxisIndex: "all" }] };
    o.series = [
      ...extra,
      ...spikes.length ? [{ name: "Spikes", type: "scatter", symbolSize: 9, itemStyle: { color: th.bad, borderColor: "#fff", borderWidth: 1 }, data: [], markLine: { silent: true, symbol: "none", lineStyle: { color: th.bad, opacity: 0.35, type: "dashed" }, label: { show: false }, data: spikes.map((s) => ({ xAxis: s.ts })) } }] : [],
      ...(sel != null ? [{ name: "Selected", type: "line", data: [], markLine: { silent: true, symbol: "none", lineStyle: { color: th.accent, width: 1.5 }, label: { formatter: timeOfDay(sel, true), color: th.accent }, data: [{ xAxis: sel }] } }] : []),
    ];
    return o;
  };
  const optUtil = useMemo(() => {
    const S: any[] = [];
    if (series.cpu) S.push(line("CPU", pts.map((p) => [p.t, p.cpu]), SERIES.cpu));
    if (series.ram) S.push(line("RAM", pts.map((p) => [p.t, p.ramUsed != null ? (p.ramUsed * 100) / total : null]), SERIES.ram));
    if (series.gpu) S.push(line("GPU", pts.map((p) => [p.t, p.gpuUtil]), SERIES.gpu));
    if (series.vram && vtotal) S.push(line("VRAM", pts.map((p) => [p.t, p.vramUsed != null ? (p.vramUsed * 100) / vtotal : null]), SERIES.vram));
    return mk(S, (v) => v + "%", (v) => v.toFixed(0) + "%", 100);
  }, [pts, spikes, sel, series, total, vtotal, ms]);
  const optMem = useMemo(
    () => mk([line("RAM in use", pts.map((p) => [p.t, p.ramUsed]), SERIES.ram, { area: true }), line("Available (lowest)", pts.map((p) => [p.t, p.ramAvailMin]), SERIES.cpu), line("Committed", pts.map((p) => [p.t, p.commit]), SERIES.commit, { lineStyle: { width: 1, type: "dashed", color: SERIES.commit } }), ...(vtotal ? [line("VRAM used", pts.map((p) => [p.t, p.vramUsed]), SERIES.vram)] : [])], (v) => bytes(v, 0), bytes, undefined, 60),
    [pts, spikes, sel, vtotal, ms],
  );
  const optTemp = useMemo(() => mk([line("GPU", pts.map((p) => [p.t, p.gpuTemp != null ? (unit === "F" ? p.gpuTemp * 1.8 + 32 : p.gpuTemp) : null]), SERIES.temp), line("CPU", pts.map((p) => [p.t, p.cpuTemp != null ? (unit === "F" ? p.cpuTemp * 1.8 + 32 : p.cpuTemp) : null]), "#fb923c")], (v) => v + "°" + unit, (v) => v.toFixed(0) + "°" + unit, "dataMax"), [pts, spikes, sel, unit, ms]);
  const optIo = useMemo(() => mk([line("Disk read", pts.map((p) => [p.t, p.diskR]), SERIES.read), line("Disk write", pts.map((p) => [p.t, p.diskW]), SERIES.write), line("Net down", pts.map((p) => [p.t, p.netRx]), SERIES.rx, { lineStyle: { width: 1.2, type: "dashed", color: SERIES.rx } }), line("Net up", pts.map((p) => [p.t, p.netTx]), SERIES.tx, { lineStyle: { width: 1.2, type: "dashed", color: SERIES.tx } })], (v) => bytes(v, 0) + "/s", rate, "dataMax", 70), [pts, spikes, sel, ms]);

  const click = (e: any, chart: any) => {
    const v = chart.convertFromPixel({ gridIndex: 0 }, [e.offsetX, e.offsetY]) as number[];
    if (v && isFinite(v[0])) {
      const p = nearest(pts, v[0]);
      inspect(p ? p.t : v[0]);
    }
  };

  const peaks = useMemo(() => {
    const best = (f: (p: Point) => number | null) => {
      let b: Point | null = null;
      let bv = -Infinity;
      for (const p of pts) {
        const v = f(p);
        if (v != null && v > bv) {
          bv = v;
          b = p;
        }
      }
      return b ? { v: bv, t: b.t } : null;
    };
    return [
      { l: "CPU usage", r: best((p) => p.cpuMax ?? p.cpu), f: (v: number) => pct(v) },
      { l: "RAM in use", r: best((p) => p.ramUsedMax ?? p.ramUsed), f: (v: number) => `${bytes(v)} (${pct((v * 100) / total)})` },
      { l: "GPU usage", r: best((p) => p.gpuUtilMax ?? p.gpuUtil), f: (v: number) => pct(v) },
      { l: "VRAM used", r: best((p) => p.vramUsedMax ?? p.vramUsed), f: (v: number) => bytes(v) },
      { l: "GPU temperature", r: best((p) => p.gpuTemp), f: (v: number) => temp(v, unit) },
      { l: "GPU power", r: best((p) => p.gpuPower), f: (v: number) => v.toFixed(0) + " W" },
      { l: "Disk read", r: best((p) => p.diskR), f: rate },
      { l: "Disk write", r: best((p) => p.diskW), f: rate },
      { l: "Download", r: best((p) => p.netRx), f: rate },
      { l: "Upload", r: best((p) => p.netTx), f: rate },
    ];
  }, [pts, total, unit]);

  const exportCsv = async () => {
    const path = await save({ defaultPath: `pulse-history-${new Date().toISOString().slice(0, 10)}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
    if (!path) return;
    try {
      const n = await api.exportCsv(Date.now() - ms, Date.now(), path);
      toast({ kind: "good", title: `Exported ${num(n)} samples`, body: path });
    } catch (e) {
      toast({ kind: "bad", title: "Export failed", body: String(e) });
    }
  };

  const selPt = sel != null ? nearest(pts, sel) : null;
  const panelDefs: { id: PanelId; title: string; opt: any; tip?: string }[] = [
    { id: "util", title: "Utilization", opt: optUtil, tip: "Each point is the average over its time bucket. RAM is shown as % of installed memory." },
    { id: "mem", title: "Memory", opt: optMem, tip: "RAM in use = total minus available. 'Available (lowest)' is the minimum seen in each bucket, so dips are not averaged away." },
    { id: "temp", title: "Temperatures", opt: optTemp, tip: "GPU from NVML. CPU appears only when a sensor provider such as LibreHardwareMonitor is running." },
    { id: "io", title: "Disk & network throughput", opt: optIo },
  ];

  return (
    <div className="h-full overflow-auto p-5 space-y-4">
      <div className="flex items-end justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-[20px] font-semibold tracking-tight">History</h1>
          <p className="text-muted text-[12.5px] mt-0.5">Locally recorded measurements. Nothing leaves this PC.</p>
        </div>
        <div className="flex items-center gap-2">
          <Segmented value={range} onChange={setRange} options={RANGES.map((r) => ({ value: r.value, label: r.label }))} />
          <Btn onClick={exportCsv} title="Export the selected time range to a CSV file">
            Export CSV
          </Btn>
        </div>
      </div>

      <div className="text-[12px] text-muted flex flex-wrap gap-x-4 gap-y-1">
        {settings && !settings.recordEnabled && <Badge tone="warn">Recording is turned off</Badge>}
        {info?.oldestTs ? (
          <span>
            Recorded since <b className="text-ink">{dateTime(info.oldestTs)}</b> · {num(info.samples)} samples · {bytes(info.sizeBytes)} on disk · keeps {settings?.retentionDays ?? 7} days
          </span>
        ) : (
          <span>No history yet. Recording begins while PULSE is running.</span>
        )}
        {info?.oldestTs && info.oldestTs > Date.now() - ms && <span className="text-warn">No data exists before {dateTime(info.oldestTs)}.</span>}
      </div>

      <div className="flex gap-4 flex-wrap items-center text-[12px] text-muted">
        <span className="uppercase tracking-wide text-[11px]">Panels</span>
        {(Object.keys(panels) as PanelId[]).map((k) => (
          <label key={k} className="flex items-center gap-1.5 cursor-pointer">
            <input type="checkbox" checked={panels[k]} onChange={(e) => setPanels({ ...panels, [k]: e.target.checked })} />
            {panelDefs.find((p) => p.id === k)!.title}
          </label>
        ))}
        <span className="flex-1" />
        <span className="text-faint">Click any chart to inspect that moment. Red dashes mark detected spikes.</span>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        {panelDefs
          .filter((p) => panels[p.id])
          .map((p) => (
            <Card key={p.id} title={p.title} tip={p.tip} right={p.id === "util" ? <div className="flex gap-2 text-[11.5px] text-muted">{(["cpu", "ram", "gpu", "vram"] as const).map((k) => <label key={k} className="flex items-center gap-1 cursor-pointer"><input type="checkbox" checked={series[k]} onChange={(e) => setSeries({ ...series, [k]: e.target.checked })} />{k.toUpperCase()}</label>)}</div> : undefined}>
              <div className="relative">
                {loading && (
                  <div className="absolute inset-0 z-10 flex items-center justify-center bg-surface/50">
                    <Spinner />
                  </div>
                )}
                <Chart option={p.opt} height={200} onZr={click} />
              </div>
            </Card>
          ))}
      </div>

      {sel != null && (
        <div className="rounded-xl border border-accent/30 bg-accentsoft/50 p-4 fade-in">
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-[11px] uppercase tracking-wide text-accent">Inspecting</div>
              <div className="text-[15px] font-semibold">{dateTime(sel)}</div>
            </div>
            <Btn small kind="ghost" onClick={() => setSel(null)}>
              Close
            </Btn>
          </div>
          {selPt && (
            <div className="grid grid-cols-2 md:grid-cols-4 xl:grid-cols-6 gap-3 mt-3 text-[12.5px] num">
              <KV l="CPU" v={pct(selPt.cpu)} />
              <KV l="RAM in use" v={`${bytes(selPt.ramUsed)} (${pct(((selPt.ramUsed ?? 0) * 100) / total)})`} />
              <KV l="Lowest available" v={bytes(selPt.ramAvailMin)} />
              <KV l="Committed" v={bytes(selPt.commit)} />
              <KV l="GPU" v={selPt.gpuUtil != null ? pct(selPt.gpuUtil) : "—"} />
              <KV l="VRAM" v={selPt.vramUsed != null ? bytes(selPt.vramUsed) : "—"} />
              <KV l="GPU temp" v={temp(selPt.gpuTemp, unit)} />
              <KV l="GPU power" v={selPt.gpuPower != null ? selPt.gpuPower.toFixed(0) + " W" : "—"} />
              <KV l="Disk R / W" v={`${rate(selPt.diskR)} / ${rate(selPt.diskW)}`} />
              <KV l="Net ↓ / ↑" v={`${rate(selPt.netRx)} / ${rate(selPt.netTx)}`} />
            </div>
          )}
          <div className="mt-3">
            {snap === undefined ? (
              <Spinner />
            ) : snap === null ? (
              <div className="text-[12.5px] text-muted">No process snapshot was recorded near this time.</div>
            ) : (
              <>
                <div className="text-[12px] text-muted mb-1.5">
                  Top applications at <b className="text-ink">{timeOfDay(snap.ts, true)}</b> (nearest recorded snapshot):
                </div>
                <AppsTable snap={snap} />
              </>
            )}
          </div>
        </div>
      )}

      <div className="grid gap-4 xl:grid-cols-3">
        <Card title="Peaks in this window" tip="Highest values found in recorded data for the selected range. Click a time to inspect it.">
          <table className="w-full text-[12.5px]">
            <tbody>
              {peaks.map((p) => (
                <tr key={p.l} className="border-b border-line/50 last:border-0">
                  <td className="py-1.5 text-muted">{p.l}</td>
                  <td className="py-1.5 num font-medium text-right">{p.r ? p.f(p.r.v) : "—"}</td>
                  <td className="py-1.5 text-right pl-3">
                    {p.r ? (
                      <button className="text-accent hover:underline num text-[11.5px]" onClick={() => inspect(p.r!.t)}>
                        {dateTime(p.r.t)}
                      </button>
                    ) : (
                      <span className="text-faint text-[11.5px]">no data</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
        <Card title={`Detected spikes (${spikes.length})`} tip="Chronological list. Memory spikes list the applications that grew most.">
          <div className="max-h-[360px] overflow-auto -mr-2 pr-2 space-y-2.5">
            {spikes.length === 0 && <div className="text-[12.5px] text-muted">None in this window.</div>}
            {spikes.map((s) => (
              <div key={s.id} className="text-[12.5px]">
                <div className="flex items-center gap-2">
                  <Badge tone={s.kind === "memory" ? "accent" : s.kind === "cpu" ? "warn" : "neutral"}>{s.kind}</Badge>
                  <button className="text-accent hover:underline num text-[11.5px]" onClick={() => inspect(s.ts)}>
                    {dateTime(s.ts)}
                  </button>
                </div>
                <div className="text-muted mt-0.5">{s.summary}</div>
              </div>
            ))}
          </div>
        </Card>
        <Card title="Alerts sent" tip="Alerts raised by your thresholds. Each has a cooldown so it cannot spam you.">
          <div className="max-h-[360px] overflow-auto space-y-2.5">
            {alerts.length === 0 && <div className="text-[12.5px] text-muted">No alerts have fired.</div>}
            {alerts.map((a) => (
              <div key={a.id} className="text-[12.5px]">
                <div className="text-faint num text-[11.5px]">{dateTime(a.ts)}</div>
                <div className="text-muted">{a.message}</div>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}

function KV({ l, v }: { l: string; v: string }) {
  return (
    <div>
      <div className="text-[11px] text-muted">{l}</div>
      <div className="font-medium">{v}</div>
    </div>
  );
}
