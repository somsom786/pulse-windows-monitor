import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { Attention, Inventory, Point, ProcRow, SpikeRow, SystemSnap } from "../lib/types";
import { bytes, dur, mhz, pct, rate, temp, timeOfDay, bits, dateTime, num } from "../lib/format";
import { Badge, Bar, Btn, Card, cx, Empty, Icon, Info, ReadingView, Segmented, Stat, useInterval } from "../components/ui";
import { baseOption, Chart, line, SERIES, theme } from "../components/Chart";

function Spark({ data, color, max = 100 }: { data: [number, number | null][]; color: string; max?: number | "dataMax" }) {
  const t = theme();
  const option = useMemo(
    () => ({
      animation: false,
      grid: { left: 0, right: 0, top: 2, bottom: 0 },
      xAxis: { type: "time", show: false },
      yAxis: { type: "value", min: 0, max, show: false },
      series: [{ type: "line", data, showSymbol: false, lineStyle: { width: 1.5, color }, areaStyle: { color, opacity: 0.14 }, silent: true }],
    }),
    [data, color, max, t.accent],
  );
  return <Chart option={option} height={44} />;
}

function Metric({ title, icon, children, spark, onToggle, open, tone }: { title: string; icon?: React.ReactNode; children: React.ReactNode; spark?: React.ReactNode; onToggle?: () => void; open?: boolean; tone?: "good" | "warn" | "bad" }) {
  return (
    <div className="rounded-xl border border-line bg-surface p-4 flex flex-col gap-2 min-w-0">
      <div className="flex items-center justify-between">
        <div className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted flex items-center gap-2">
          {icon}
          {title}
        </div>
        {onToggle && (
          <button className="text-[11.5px] text-muted hover:text-accent" onClick={onToggle}>
            {open ? "Hide details" : "Details"}
          </button>
        )}
      </div>
      {children}
      {spark}
    </div>
  );
}

export function Overview() {
  const snap = useStore((s) => s.snap);
  const live = useStore((s) => s.live);
  const settings = useStore((s) => s.settings);
  const navigate = useStore((s) => s.navigate);
  const [inv, setInv] = useState<Inventory | null>(null);
  const [att, setAtt] = useState<Attention[] | null>(null);
  const [spikes, setSpikes] = useState<SpikeRow[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [range, setRange] = useState<"live" | "1h" | "6h" | "24h">("live");
  const [hist, setHist] = useState<Point[]>([]);
  const unit = settings?.tempUnit ?? "C";

  useEffect(() => {
    api.hardware().then(setInv).catch(() => {});
  }, []);
  const loadAtt = useCallback(() => {
    api.attention().then(setAtt).catch(() => setAtt([]));
    api.historySpikes(Date.now() - 24 * 3600_000, Date.now(), 5).then(setSpikes).catch(() => {});
  }, []);
  useEffect(loadAtt, [loadAtt]);
  useInterval(loadAtt, 20000);

  const loadHist = useCallback(() => {
    if (range === "live") return;
    const ms = range === "1h" ? 3600_000 : range === "6h" ? 6 * 3600_000 : 24 * 3600_000;
    api.historySeries(Date.now() - ms, Date.now(), 500).then(setHist).catch(() => {});
  }, [range]);
  useEffect(loadHist, [loadHist]);
  useInterval(loadHist, 30000, [range]);

  if (!snap) {
    return (
      <div className="p-6 grid grid-cols-3 gap-4">
        {[0, 1, 2, 3, 4, 5].map((i) => (
          <div key={i} className="skeleton h-40" />
        ))}
      </div>
    );
  }

  const s = snap;
  const g = s.gpu;
  const liveSlice = live.slice(-450);
  // sparklines are tiny: one point per ~10 s is plenty and keeps redraw cost negligible
  const thin = <T,>(a: T[], n = 90): T[] => (a.length <= n ? a : a.filter((_, i) => i % Math.ceil(a.length / n) === a.length % Math.ceil(a.length / n) || i === a.length - 1));
  const cpuSpark = thin(liveSlice).map((p) => [p.t, p.cpu] as [number, number]);
  const ramSpark = thin(liveSlice).map((p) => [p.t, p.ramPct] as [number, number]);
  const gpuSpark = thin(liveSlice).map((p) => [p.t, p.gpu] as [number, number | null]);
  const diskSpark = thin(liveSlice).map((p) => [p.t, (p.diskR ?? 0) + (p.diskW ?? 0)] as [number, number]);
  const netSpark = thin(liveSlice).map((p) => [p.t, (p.netRx ?? 0) + (p.netTx ?? 0)] as [number, number]);
  const ramUsedPct = ((s.mem.total - s.mem.available) * 100) / s.mem.total;
  const vramPct = g && g.vramUsed.value != null && g.vramTotal.value ? (g.vramUsed.value * 100) / g.vramTotal.value : null;
  const toneFor = (v: number, w = 75, b = 90) => (v >= b ? "bad" : v >= w ? "warn" : undefined);

  return (
    <div className="h-full overflow-auto p-5 space-y-4">
      <div className="flex items-end justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-[20px] font-semibold tracking-tight">Overview</h1>
          <p className="text-[12.5px] text-muted mt-0.5 num selectable">
            {inv?.os ? `${inv.os} · ${inv.osBuild}` : "Windows"} · {s.cpu.name} · {s.cpu.physical}C/{s.cpu.logical}T · {bytes(s.mem.total, 0)} RAM{g ? ` · ${g.name}${g.vramTotal.value ? ` ${bytes(g.vramTotal.value, 0)}` : ""}` : ""} · up {dur(s.uptimeS)}
          </p>
        </div>
        {inv?.board && <span className="text-[11.5px] text-faint selectable">{inv.board}</span>}
      </div>

      <Attn items={att} navigate={navigate} />

      <div className="grid gap-4 grid-cols-1 md:grid-cols-2 xl:grid-cols-3">
        {/* CPU */}
        <Metric title="CPU" icon={<Icon.hardware width={14} height={14} />} onToggle={() => setOpen(open === "cpu" ? null : "cpu")} open={open === "cpu"} spark={<Spark data={cpuSpark} color={SERIES.cpu} />}>
          <div className="flex items-baseline gap-2">
            <span className={cx("text-[30px] font-semibold num leading-none", toneFor(s.cpu.total) === "bad" && "text-bad", toneFor(s.cpu.total) === "warn" && "text-warn")}>{s.cpu.total.toFixed(0)}%</span>
            <span className="text-muted text-[12px]">total usage</span>
          </div>
          <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
            <Stat label="Frequency" value={s.cpu.freqMhz != null ? mhz(s.cpu.freqMhz) : "—"} sub={s.cpu.baseMhz ? `base ${mhz(s.cpu.baseMhz)}` : undefined} tip="Estimated from the Windows 'Processor Performance' counter × base clock. It reflects boost and power-saving states." />
            <Stat label="Temperature" value={<ReadingView r={s.cpuTemp} fmt={(v) => temp(v, unit)} />} sub={s.cpuTemp.value != null ? s.cpuTemp.note ?? undefined : undefined} />
            <Stat label="Cores / threads" value={`${s.cpu.physical} / ${s.cpu.logical}`} />
            <Stat label="Processes" value={num(s.mem.processes)} sub={`${num(s.mem.threads)} threads`} />
          </div>
          {open === "cpu" && (
            <div className="pt-1 fade-in">
              <div className="text-[11px] text-muted mb-1.5">Per logical processor</div>
              <div className="grid grid-cols-4 gap-1.5">
                {s.cpu.perCore.map((c, i) => (
                  <div key={i} title={`CPU ${i}: ${c.toFixed(0)}%${s.cpu.perCoreFreq[i] != null ? " · " + mhz(s.cpu.perCoreFreq[i]) : ""}`} className="rounded-md bg-surface2 px-1.5 py-1">
                    <div className="flex justify-between text-[10px] text-faint num">
                      <span>{i}</span>
                      <span>{c.toFixed(0)}%</span>
                    </div>
                    <Bar value={c} height={4} tone="auto" />
                  </div>
                ))}
              </div>
            </div>
          )}
        </Metric>

        {/* GPU */}
        <Metric title="GPU" icon={<Icon.hardware width={14} height={14} />} onToggle={g ? () => setOpen(open === "gpu" ? null : "gpu") : undefined} open={open === "gpu"} spark={g ? <Spark data={gpuSpark} color={SERIES.gpu} /> : undefined}>
          {g ? (
            <>
              <div className="flex items-baseline gap-2">
                <span className="text-[30px] font-semibold num leading-none">{g.util.value != null ? g.util.value.toFixed(0) + "%" : "—"}</span>
                <span className="text-muted text-[12px] truncate">{g.name.replace("NVIDIA GeForce ", "")}</span>
              </div>
              <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
                <Stat label="Temperature" value={<ReadingView r={g.temp} fmt={(v) => temp(v, unit)} />} tone={g.temp.value != null && g.tempSlowdown.value != null && g.temp.value >= g.tempSlowdown.value - 5 ? "warn" : undefined} />
                <Stat label="VRAM" value={g.vramUsed.value != null ? `${bytes(g.vramUsed.value)} / ${bytes(g.vramTotal.value, 0)}` : "—"} sub={vramPct != null ? pct(vramPct) + " used" : undefined} />
                <Stat label="Power" value={<ReadingView r={g.powerW} fmt={(v) => v.toFixed(0) + " W"} />} sub={g.powerLimitW.value != null ? `limit ${g.powerLimitW.value.toFixed(0)} W` : undefined} />
                <Stat label="Fan" value={g.fanRpm.value != null ? `${num(g.fanRpm.value)} RPM` : g.fanPct.value != null ? `${g.fanPct.value.toFixed(0)}% (duty)` : <ReadingView r={g.fanPct} fmt={(v) => v + "%"} />} sub={g.fanRpm.value == null && g.fanPct.value != null ? "RPM not reported by driver" : undefined} tip="NVML reports fan speed as a percentage of maximum; RPM is shown only when the driver reports it." />
              </div>
              {open === "gpu" && (
                <div className="grid grid-cols-2 gap-x-3 gap-y-1.5 pt-1 fade-in">
                  <Stat label="Memory controller" value={g.memCtrlUtil.value != null ? pct(g.memCtrlUtil.value) : "—"} tip="Share of time the GPU memory interface was busy (not VRAM capacity)." />
                  <Stat label="Core clock" value={g.clockGpu.value != null ? mhz(g.clockGpu.value) : "—"} sub={g.clockGpuMax.value != null ? `max ${mhz(g.clockGpuMax.value)}` : undefined} />
                  <Stat label="Memory clock" value={g.clockMem.value != null ? mhz(g.clockMem.value) : "—"} />
                  <Stat label="Throttling" value={g.throttle.length ? g.throttle.join(", ") : "None reported"} tone={g.throttle.some((t) => t.includes("thermal")) ? "warn" : undefined} />
                </div>
              )}
            </>
          ) : (
            <Empty title="No NVIDIA GPU data" body={s.gpuStatus} />
          )}
        </Metric>

        {/* RAM */}
        <Metric title="Memory" icon={<Icon.memory width={14} height={14} />} onToggle={() => navigate("processes")} open={false} spark={<Spark data={ramSpark} color={SERIES.ram} />}>
          <div className="flex items-baseline gap-2">
            <span className={cx("text-[30px] font-semibold num leading-none", toneFor(ramUsedPct, 85, 93) === "bad" && "text-bad", toneFor(ramUsedPct, 85, 93) === "warn" && "text-warn")}>{ramUsedPct.toFixed(0)}%</span>
            <span className="text-muted text-[12px] num">
              {bytes(s.mem.total - s.mem.available)} of {bytes(s.mem.total, 0)}
            </span>
          </div>
          <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
            <Stat label="Available" value={bytes(s.mem.available)} sub="free + reusable cache" tip="Memory Windows can hand out immediately. Includes the standby cache." />
            <Stat label="Pressure" value={<Badge tone={s.mem.pressure === "normal" ? "good" : s.mem.pressure === "elevated" ? "warn" : "bad"}>{s.mem.pressure}</Badge>} sub={`commit ${((s.mem.commitTotal * 100) / s.mem.commitLimit).toFixed(0)}%`} tip={s.mem.pressureReason} />
            <Stat label="Cached" value={s.mem.standby != null ? bytes(s.mem.standby + (s.mem.modified ?? 0)) : "—"} sub="standby + modified" />
            <Stat label="Pagefile" value={s.mem.pagefiles.length ? bytes(s.mem.pagefiles.reduce((a, p) => a + p.inUse, 0)) : "none"} sub={s.mem.pagefiles.length ? `of ${bytes(s.mem.pagefiles.reduce((a, p) => a + p.total, 0), 0)}` : undefined} />
          </div>
        </Metric>

        {/* Storage */}
        <Metric title="Storage" icon={<Icon.drive width={14} height={14} />} onToggle={() => navigate("storage")} open={false}>
          <div className="space-y-2.5">
            {s.disks.map((d) => {
              const used = d.total - d.free;
              const p = (used * 100) / d.total;
              return (
                <div key={d.letter}>
                  <div className="flex justify-between text-[12.5px]">
                    <span className="font-medium">
                      {d.letter} <span className="text-faint font-normal">{d.label}</span>
                    </span>
                    <span className="num text-muted">
                      <b className={cx("font-semibold", p > 90 ? "text-bad" : p > 80 ? "text-warn" : "text-ink")}>{bytes(d.free)}</b> free of {bytes(d.total, 0)}
                    </span>
                  </div>
                  <Bar value={used} max={d.total} tone="auto" className="mt-1" />
                  <div className="flex justify-between text-[11px] text-faint num mt-0.5">
                    <span>{p.toFixed(0)}% used</span>
                    <span title="Time the disk was busy servicing requests. Can briefly exceed what the throughput suggests on NVMe.">
                      {d.activePct != null ? `active ${d.activePct.toFixed(0)}%` : "activity —"} · R {rate(d.readBps)} · W {rate(d.writeBps)}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        </Metric>

        {/* Disk I/O */}
        <Metric title="Disk activity" icon={<Icon.drive width={14} height={14} />} spark={<Spark data={diskSpark} color={SERIES.read} max="dataMax" />}>
          <div className="grid grid-cols-2 gap-3">
            <Stat label="Read" value={<span className="text-[20px]">{rate(s.diskIo.readBps)}</span>} />
            <Stat label="Write" value={<span className="text-[20px]">{rate(s.diskIo.writeBps)}</span>} />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <Stat label="Busy time" value={s.diskIo.activePct != null ? pct(s.diskIo.activePct) : "—"} tip="Percentage of time the physical disks were busy servicing requests (all drives)." tone={s.diskIo.activePct != null && s.diskIo.activePct > 90 ? "warn" : undefined} />
            <Stat label="Latency" value={s.disks.find((d) => d.latencyMs != null)?.latencyMs != null ? s.disks.find((d) => d.latencyMs != null)!.latencyMs!.toFixed(2) + " ms" : "—"} sub={s.disks.find((d) => d.latencyMs != null)?.letter} tip="Average time per disk transfer on the first drive." />
          </div>
        </Metric>

        {/* Network */}
        <Metric title="Network" icon={<Icon.overview width={14} height={14} />} spark={<Spark data={netSpark} color={SERIES.rx} max="dataMax" />}>
          <div className="grid grid-cols-2 gap-3">
            <Stat label="Download" value={<span className="text-[20px]">{rate(s.net.rxBps)}</span>} sub={bits(s.net.rxBps)} />
            <Stat label="Upload" value={<span className="text-[20px]">{rate(s.net.txBps)}</span>} sub={bits(s.net.txBps)} />
          </div>
          <div className="space-y-1 pt-1">
            {s.net.adapters.slice(0, 3).map((a) => {
              const ia = inv?.adapters.find((x) => x.description === a.name || a.name.startsWith(x.description.slice(0, 20)) || x.description.startsWith(a.name.slice(0, 20)));
              return (
                <div key={a.name} className="text-[11.5px] flex justify-between gap-2" title={ia ? `${ia.ipv4.join(", ")} · ${ia.mac}` : a.name}>
                  <span className="truncate text-muted">{a.name}</span>
                  <span className="num text-faint shrink-0">{ia?.ipv4?.[0] ?? (a.linkBps ? bits(a.linkBps / 8) : "")}</span>
                </div>
              );
            })}
          </div>
        </Metric>
      </div>

      <div className="grid gap-4 xl:grid-cols-3">
        <Card title="Activity" className="xl:col-span-2" right={<Segmented value={range} onChange={setRange} options={[{ value: "live", label: "Live 15 min" }, { value: "1h", label: "1 h" }, { value: "6h", label: "6 h" }, { value: "24h", label: "24 h" }]} />}>
          <ActivityChart range={range} live={liveSlice} hist={hist} />
          {range !== "live" && hist.length === 0 && <div className="text-[12px] text-muted">No recorded data in this window yet. Recording started when PULSE first ran with recording enabled.</div>}
        </Card>
        <TopProcs />
      </div>

      <div className="grid gap-4 xl:grid-cols-3">
        <Card title="Disk & network throughput" className="xl:col-span-2" tip="Live throughput over the last 15 minutes.">
          <IoChart live={liveSlice} />
        </Card>
        <Card title="Recent spikes" tip="Resource spikes PULSE detected while running. Each one stores which applications were responsible." right={<button className="text-[11.5px] text-accent hover:underline" onClick={() => navigate("processes", { tab: "spikes" })}>Investigate</button>}>
          {spikes.length === 0 ? (
            <div className="text-[12.5px] text-muted py-3">No spikes detected in the last 24 hours of recorded data.</div>
          ) : (
            <div className="space-y-2">
              {spikes.map((sp) => (
                <div key={sp.id} className="text-[12.5px]">
                  <div className="flex items-center gap-2">
                    <Badge tone={sp.kind === "memory" ? "accent" : "warn"}>{sp.kind}</Badge>
                    <span className="text-faint num">{dateTime(sp.ts)}</span>
                  </div>
                  <div className="text-muted mt-0.5 line-clamp-2">{sp.summary}</div>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

function Attn({ items, navigate }: { items: Attention[] | null; navigate: (v: any, o?: any) => void }) {
  if (!items) return <div className="skeleton h-16" />;
  const warn = items.filter((i) => i.severity !== "info").length;
  return (
    <Card title="Needs attention" right={warn === 0 ? <Badge tone="good">Nothing urgent</Badge> : <Badge tone="warn">{warn} to look at</Badge>} tip="Only facts backed by actual measurements. There is no health score — each line tells you what was measured.">
      {items.length === 0 ? (
        <div className="text-[12.5px] text-muted py-1">Nothing to report yet. Observations appear as PULSE collects data.</div>
      ) : (
        <div className="grid gap-2 md:grid-cols-2">
          {items.map((a, i) => (
            <button key={i} onClick={() => a.view && navigate(a.view)} className="text-left flex gap-3 rounded-lg border border-line bg-surface2 px-3 py-2 hover:border-line2 transition-colors">
              <span className={cx("mt-1.5 w-2 h-2 rounded-full shrink-0", a.severity === "crit" ? "bg-bad" : a.severity === "warn" ? "bg-warn" : "bg-accent")} />
              <span className="min-w-0">
                <span className="block text-[13px] font-medium">
                  {a.title}
                  {a.ts && <span className="text-faint font-normal text-[11.5px] ml-2 num">{dateTime(a.ts)}</span>}
                </span>
                <span className="block text-[12px] text-muted">{a.detail}</span>
              </span>
            </button>
          ))}
        </div>
      )}
    </Card>
  );
}

function ActivityChart({ range, live, hist }: { range: string; live: ReturnType<typeof useStore.getState>["live"]; hist: Point[] }) {
  const t = theme();
  const [show, setShow] = useState({ cpu: true, ram: true, gpu: true, vram: false });
  const total = useStore((s) => s.snap?.mem.total) ?? 1;
  const vtotal = useStore((s) => s.snap?.gpu?.vramTotal.value) ?? null;
  const option = useMemo(() => {
    const o: any = baseOption(t, { yFormatter: (v: number) => v + "%", yMax: 100, legend: true });
    o.tooltip.valueFormatter = (v: number) => (v == null ? "—" : v.toFixed(0) + "%");
    const S: any[] = [];
    if (range === "live") {
      if (show.cpu) S.push(line("CPU", live.map((p) => [p.t, p.cpu]), SERIES.cpu));
      if (show.ram) S.push(line("RAM", live.map((p) => [p.t, p.ramPct]), SERIES.ram));
      if (show.gpu) S.push(line("GPU", live.map((p) => [p.t, p.gpu]), SERIES.gpu));
      if (show.vram) S.push(line("VRAM", live.map((p) => [p.t, p.vramPct]), SERIES.vram));
    } else {
      if (show.cpu) S.push(line("CPU", hist.map((p) => [p.t, p.cpu]), SERIES.cpu));
      if (show.ram) S.push(line("RAM", hist.map((p) => [p.t, p.ramUsed != null ? (p.ramUsed * 100) / total : null]), SERIES.ram));
      if (show.gpu) S.push(line("GPU", hist.map((p) => [p.t, p.gpuUtil]), SERIES.gpu));
      if (show.vram && vtotal) S.push(line("VRAM", hist.map((p) => [p.t, p.vramUsed != null ? (p.vramUsed * 100) / vtotal : null]), SERIES.vram));
    }
    o.series = S;
    return o;
  }, [range, live, hist, show, total, vtotal, t.accent]);
  return (
    <div>
      <div className="flex gap-3 text-[12px] mb-1">
        {(["cpu", "ram", "gpu", "vram"] as const).map((k) => (
          <label key={k} className="flex items-center gap-1.5 cursor-pointer text-muted">
            <input type="checkbox" checked={show[k]} onChange={(e) => setShow({ ...show, [k]: e.target.checked })} />
            {k.toUpperCase()}
          </label>
        ))}
      </div>
      <Chart option={option} height={210} />
    </div>
  );
}

function IoChart({ live }: { live: ReturnType<typeof useStore.getState>["live"] }) {
  const t = theme();
  const option = useMemo(() => {
    const o: any = baseOption(t, { yFormatter: (v: number) => bytes(v, 0) + "/s", yMax: "dataMax", legend: true, gridLeft: 64 });
    o.tooltip.valueFormatter = (v: number) => (v == null ? "—" : rate(v));
    o.series = [line("Disk read", live.map((p) => [p.t, p.diskR]), SERIES.read), line("Disk write", live.map((p) => [p.t, p.diskW]), SERIES.write), line("Net down", live.map((p) => [p.t, p.netRx]), SERIES.rx, { lineStyle: { width: 1.2, type: "dashed", color: SERIES.rx } }), line("Net up", live.map((p) => [p.t, p.netTx]), SERIES.tx, { lineStyle: { width: 1.2, type: "dashed", color: SERIES.tx } })];
    return o;
  }, [live, t.accent]);
  return <Chart option={option} height={190} />;
}

function TopProcs() {
  const apps = useStore((s) => s.topApps);
  const [by, setBy] = useState<"mem" | "cpu" | "gpu">("mem");
  const navigate = useStore((s) => s.navigate);
  const top = useMemo(() => {
    const arr = [...apps];
    arr.sort((a, b) => (by === "mem" ? b.mem - a.mem : by === "cpu" ? b.cpu - a.cpu : b.gpu - a.gpu || b.gpuMem - a.gpuMem));
    return arr.slice(0, 5);
  }, [apps, by]);
  const max = top[0] ? (by === "mem" ? top[0].mem : by === "cpu" ? top[0].cpu : top[0].gpu || 1) : 1;
  return (
    <Card title="Top processes" right={<Segmented value={by} onChange={setBy} options={[{ value: "mem", label: "Memory" }, { value: "cpu", label: "CPU" }, { value: "gpu", label: "GPU" }]} />} tip="Applications ranked with all of their processes combined. Memory is private working set.">
      {apps.length === 0 ? (
        <div className="space-y-2">
          {[0, 1, 2, 3, 4].map((i) => (
            <div key={i} className="skeleton h-8" />
          ))}
        </div>
      ) : (
        <div className="space-y-2.5">
          {top.map((p) => {
            const v = by === "mem" ? p.mem : by === "cpu" ? p.cpu : p.gpu;
            return (
              <div key={p.name}>
                <div className="flex justify-between text-[12.5px]">
                  <span className="truncate">
                    {p.name} {p.count > 1 && <span className="text-faint">×{p.count}</span>}
                  </span>
                  <span className="num font-medium">{by === "mem" ? bytes(p.mem) : by === "cpu" ? p.cpu.toFixed(1) + "%" : p.gpu.toFixed(1) + "%" + (p.gpuMem ? ` · ${bytes(p.gpuMem, 0)}` : "")}</span>
                </div>
                <Bar value={v} max={Math.max(max, 0.01)} tone={by === "mem" ? "#a78bfa" : by === "cpu" ? SERIES.cpu : SERIES.gpu} className="mt-1" />
              </div>
            );
          })}
          <button className="text-[12px] text-accent hover:underline pt-1" onClick={() => navigate("processes")}>
            Open full process list →
          </button>
        </div>
      )}
    </Card>
  );
}
