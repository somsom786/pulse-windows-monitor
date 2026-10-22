import { ReactNode, useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { HubData, Inventory, Reading } from "../lib/types";
import { bytes, bits, mhz, num, pct, rate, temp, dur } from "../lib/format";
import { Badge, Bar, Btn, Card, cx, Info, ReadingView, Spinner, useInterval } from "../components/ui";

function Row({ l, children, tip }: { l: string; children: ReactNode; tip?: string }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5 border-b border-line/50 last:border-0 text-[12.5px]">
      <span className="text-muted flex items-center gap-1.5">
        {l}
        {tip && <Info text={tip} />}
      </span>
      <span className="num text-right font-medium">{children}</span>
    </div>
  );
}

export function Hardware() {
  const { snap, settings } = useStore();
  const unit = settings?.tempUnit ?? "C";
  const [inv, setInv] = useState<Inventory | null>(null);
  const [hub, setHub] = useState<HubData | null>(null);
  const [err, setErr] = useState("");
  const [lhm, setLhm] = useState<{ installed: boolean; path: string } | null>(null);
  const toast = useStore((s) => s.toast);
  useEffect(() => {
    api.hardware().then(setInv).catch((e) => setErr(String(e)));
    api.sensorHub().then(setHub);
    api.lhmInfo().then(setLhm).catch(() => {});
  }, []);
  useInterval(() => api.sensorHub().then(setHub), 5000);
  const refreshInv = () => {
    setInv(null);
    api.hardware().then(setInv);
  };
  if (!snap) return <div className="p-6"><Spinner /></div>;
  const s = snap;
  const g = s.gpu;
  const hubOk = hub?.status === "ok";
  const hubTemps = hub?.sensors.filter((x) => x.kind === "Temperature") ?? [];
  const hubFans = hub?.sensors.filter((x) => x.kind === "Fan") ?? [];
  const hubPower = hub?.sensors.filter((x) => x.kind === "Power") ?? [];

  return (
    <div className="h-full overflow-auto p-5 space-y-4">
      <div>
        <h1 className="text-[20px] font-semibold tracking-tight">Hardware</h1>
        <p className="text-muted text-[12.5px] mt-0.5">Every reading shows where it came from. Anything a sensor cannot report is labeled — PULSE never substitutes a zero.</p>
      </div>

      <div className="grid gap-4 xl:grid-cols-2">
        <Card title="CPU">
          <div className="text-[15px] font-semibold mb-2 selectable">{s.cpu.name}</div>
          <Row l="Cores / threads">{s.cpu.physical} / {s.cpu.logical}</Row>
          <Row l="Total utilization">{pct(s.cpu.total, 1)}</Row>
          <Row l="Frequency" tip="Base clock × Windows' 'Processor Performance' percentage.">{s.cpu.freqMhz != null ? `${mhz(s.cpu.freqMhz)} (base ${mhz(s.cpu.baseMhz)})` : "—"}</Row>
          <Row l="Package temperature" tip="Reading from CPU thermal sensors. Windows exposes these only through a kernel driver; PULSE reads them from LibreHardwareMonitor when it is running.">
            <ReadingView r={s.cpuTemp} fmt={(v) => temp(v, unit)} />
          </Row>
          <Row l="Power draw"><ReadingView r={s.cpuPower} fmt={(v) => v.toFixed(0) + " W"} /></Row>
          <Row l="Thermal throttling">{s.cpuTemp.value != null ? "Judged from sensor data" : <Badge>Unknown — no temperature sensor</Badge>}</Row>
          <div className="mt-3 text-[11px] uppercase tracking-wide text-muted">Per logical processor</div>
          <div className="mt-1.5 grid grid-cols-2 sm:grid-cols-4 gap-1.5">
            {s.cpu.perCore.map((c, i) => (
              <div key={i} className="rounded-md bg-surface2 px-2 py-1.5">
                <div className="flex justify-between text-[10.5px] num text-faint">
                  <span>CPU {i}</span>
                  <span>{s.cpu.perCoreFreq[i] != null ? mhz(s.cpu.perCoreFreq[i]) : ""}</span>
                </div>
                <div className="flex items-center gap-2">
                  <Bar value={c} height={4} tone="auto" />
                  <span className="text-[11px] num w-8 text-right">{c.toFixed(0)}%</span>
                </div>
              </div>
            ))}
          </div>
          {hubOk && hubTemps.filter((t) => t.identifier.includes("cpu")).length > 1 && (
            <>
              <div className="mt-3 text-[11px] uppercase tracking-wide text-muted">Core temperatures ({hub!.source})</div>
              <div className="mt-1 grid grid-cols-2 gap-x-6">
                {hubTemps
                  .filter((t) => t.identifier.includes("cpu"))
                  .map((t) => (
                    <div key={t.identifier} className="flex justify-between text-[12.5px] py-0.5">
                      <span className="text-muted">{t.name}</span>
                      <span className="num">{temp(t.value, unit)}</span>
                    </div>
                  ))}
              </div>
            </>
          )}
        </Card>

        <Card title="NVIDIA GPU">
          {g ? (
            <>
              <div className="text-[15px] font-semibold mb-2 selectable">{g.name}</div>
              <Row l="Driver">{g.driver || "—"}</Row>
              <Row l="Utilization"><ReadingView r={g.util} fmt={(v) => pct(v)} /></Row>
              <Row l="Memory controller load" tip="How busy the GPU memory interface is, not how full VRAM is."><ReadingView r={g.memCtrlUtil} fmt={(v) => pct(v)} /></Row>
              <Row l="VRAM used / total">{g.vramUsed.value != null ? `${bytes(g.vramUsed.value)} / ${bytes(g.vramTotal.value)}` : "—"}</Row>
              <Row l="Temperature (GPU core)"><ReadingView r={g.temp} fmt={(v) => temp(v, unit)} /></Row>
              <Row l="Slowdown threshold" tip="The temperature at which the GPU starts reducing clocks."><ReadingView r={g.tempSlowdown} fmt={(v) => temp(v, unit)} /></Row>
              <Row l="Hotspot temperature"><ReadingView r={g.hotspot} fmt={(v) => temp(v, unit)} /></Row>
              <Row l="Fan speed">
                {g.fanRpm.value != null ? `${num(g.fanRpm.value)} RPM` : g.fanPct.value != null ? `${g.fanPct.value.toFixed(0)}% duty (RPM not reported)` : <ReadingView r={g.fanPct} fmt={(v) => v + "%"} />}
              </Row>
              <Row l="GPU clock"><ReadingView r={g.clockGpu} fmt={mhz} /> <span className="text-faint">/ max {g.clockGpuMax.value != null ? mhz(g.clockGpuMax.value) : "—"}</span></Row>
              <Row l="Memory clock"><ReadingView r={g.clockMem} fmt={mhz} /> <span className="text-faint">/ max {g.clockMemMax.value != null ? mhz(g.clockMemMax.value) : "—"}</span></Row>
              <Row l="Power draw / limit">
                <ReadingView r={g.powerW} fmt={(v) => v.toFixed(0) + " W"} /> <span className="text-faint">/ {g.powerLimitW.value != null ? g.powerLimitW.value.toFixed(0) + " W" : "—"}</span>
              </Row>
              <Row l="Performance state">{g.pstate ?? "—"}</Row>
              <Row l="Throttling" tip={g.throttleNote}>{g.throttle.length ? <Badge tone="warn">{g.throttle.join(", ")}</Badge> : <Badge tone="good">None reported</Badge>}</Row>
              <p className="text-[11.5px] text-faint mt-2">Per-process GPU usage and VRAM are on the Processes page (Windows GPU counters).</p>
            </>
          ) : (
            <div className="text-muted text-[13px]">No NVIDIA GPU telemetry: {s.gpuStatus}</div>
          )}
        </Card>

        <Card title="Memory" tip="Installed modules, from SMBIOS.">
          <Row l="Installed">{inv?.ramDesc || `${bytes(s.mem.total, 0)}`}</Row>
          {inv?.ramModules.map((m, i) => (
            <Row key={i} l={`Module ${i + 1}`}>{m}</Row>
          ))}
          <Row l="In use / total">{bytes(s.mem.total - s.mem.available)} / {bytes(s.mem.total)}</Row>
          <Row l="Handles / processes / threads">{num(s.mem.handles)} / {num(s.mem.processes)} / {num(s.mem.threads)}</Row>
        </Card>

        <Card title="System" right={<Btn small kind="ghost" onClick={refreshInv}>Refresh</Btn>}>
          {!inv ? (
            <div className="py-3"><Spinner /></div>
          ) : (
            <>
              <Row l="Windows">{inv.os} {inv.osBuild}</Row>
              <Row l="Machine">{inv.machine || "—"}</Row>
              <Row l="Motherboard">{inv.board || "—"}</Row>
              <Row l="BIOS">{inv.bios || "—"}</Row>
              <Row l="Uptime">{dur(s.uptimeS)}</Row>
            </>
          )}
          {err && <div className="text-bad text-[12px]">{err}</div>}
        </Card>
      </div>

      <Card title="Storage hardware" tip="Model, type and health from Windows Storage Management. Temperature and SMART counters are reported only by drives and drivers that expose them to Windows, and some require administrator rights.">
        {!inv ? (
          <div className="py-3"><Spinner /></div>
        ) : inv.disks.length === 0 ? (
          <div className="text-muted text-[12.5px]">Windows did not return physical disk information.</div>
        ) : (
          <div className="grid gap-4 lg:grid-cols-2">
            {inv.disks.map((d, i) => {
              const vols = s.disks.filter((v) => d.letters.includes(v.letter));
              const io = vols[0];
              return (
                <div key={i} className="rounded-lg border border-line p-3">
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-semibold selectable">{d.model}</div>
                    <Badge tone={d.health === "Healthy" ? "good" : d.health === "Unknown" ? "neutral" : "warn"}>{d.health}</Badge>
                  </div>
                  <div className="text-[12px] text-muted mb-1.5">{d.media} · {bytes(d.size, 0)}{d.firmware ? ` · fw ${d.firmware}` : ""}{d.letters.length ? ` · ${d.letters.join(" ")}` : ""}</div>
                  {vols.map((v) => (
                    <div key={v.letter} className="mb-1.5">
                      <div className="flex justify-between text-[12px]"><span>{v.letter} {v.label}</span><span className="num text-muted">{bytes(v.total - v.free)} used · {bytes(v.free)} free · {v.fs}</span></div>
                      <Bar value={v.total - v.free} max={v.total} tone="auto" className="mt-1" />
                    </div>
                  ))}
                  <Row l="Current throughput">{io ? `R ${rate(io.readBps)} · W ${rate(io.writeBps)}` : "—"}</Row>
                  <Row l="Disk activity" tip="Percentage of time the disk was busy.">{io?.activePct != null ? pct(io.activePct) : "—"}</Row>
                  <Row l="Latency">{io?.latencyMs != null ? io.latencyMs.toFixed(2) + " ms" : "—"}</Row>
                  <Row l="Temperature">{d.temperature != null ? temp(d.temperature, unit) : <Badge tone="warn" >{d.reliabilityStatus.includes("admin") ? "Needs elevation" : "Not reported"}</Badge>}</Row>
                  <Row l="Power-on hours">{d.powerOnHours != null ? num(d.powerOnHours) : "—"}</Row>
                  <Row l="Wear">{d.wear != null ? d.wear + "%" : "—"}</Row>
                  <Row l="Read / write errors">{d.readErrors != null ? `${num(d.readErrors)} / ${num(d.writeErrors ?? 0)}` : "—"}</Row>
                  {d.temperature == null && d.powerOnHours == null && <div className="text-[11.5px] text-faint mt-1">{d.reliabilityStatus}</div>}
                </div>
              );
            })}
          </div>
        )}
      </Card>

      <Card title="Network adapters">
        {!inv ? (
          <div className="py-3"><Spinner /></div>
        ) : (
          <div className="grid gap-3 lg:grid-cols-2">
            {[...inv.adapters].filter((a) => a.status === "Connected").map((a) => {
              const live = s.net.adapters.find((x) => x.name.startsWith(a.description.slice(0, 18)) || a.description.startsWith(x.name.slice(0, 18)));
              return (
                <div key={a.name} className="rounded-lg border border-line p-3">
                  <div className="flex justify-between"><span className="font-medium">{a.name}</span><Badge tone={a.status === "Connected" ? "good" : "neutral"}>{a.status}</Badge></div>
                  <div className="text-[12px] text-muted mb-1">{a.description}</div>
                  <Row l="Link speed">{a.linkSpeedBps ? bits(a.linkSpeedBps / 8) : "—"}</Row>
                  <Row l="Current traffic">{live ? `↓ ${rate(live.rxBps)} ↑ ${rate(live.txBps)}` : "—"}</Row>
                  <Row l="IPv4"><span className="selectable">{a.ipv4.join(", ") || "—"}</span></Row>
                  <Row l="Gateway"><span className="selectable">{a.gateway.join(", ") || "—"}</span></Row>
                  <Row l="DNS"><span className="selectable">{a.dns.join(", ") || "—"}</span></Row>
                  <Row l="MAC"><span className="selectable">{a.mac || "—"}</span></Row>
                  <Row l="DHCP">{a.dhcp == null ? "—" : a.dhcp ? "Enabled" : "Static"}</Row>
                </div>
              );
            })}
            {inv.adapters.length === 0 && <div className="text-muted text-[12.5px]">No adapters returned.</div>}
            {inv.adapters.some((a) => a.status !== "Connected") && (
              <div className="lg:col-span-2 text-[12px] text-muted">
                Not connected: {inv.adapters.filter((a) => a.status !== "Connected").map((a) => `${a.name} (${a.description})`).join(" · ")}
              </div>
            )}
          </div>
        )}
      </Card>

      <Card title="Motherboard, fans & other sensors" right={<Badge tone={hubOk ? "good" : "warn"}>{hubOk ? `via ${hub!.source}` : "no provider"}</Badge>}>
        {hubOk ? (
          <div className="grid gap-4 lg:grid-cols-3">
            <SensorList title="Temperatures" items={hubTemps.map((t) => ({ n: `${t.parent ? t.parent.split("/").pop() + " · " : ""}${t.name}`, v: temp(t.value, unit) }))} />
            <SensorList title="Fans" items={hubFans.map((t) => ({ n: t.name, v: `${num(t.value)} RPM` }))} empty="No fan sensors reported (motherboard may not expose them)." />
            <SensorList title="Power" items={hubPower.map((t) => ({ n: t.name, v: `${t.value.toFixed(1)} W` }))} empty="No power sensors reported." />
          </div>
        ) : (
          <div className="text-[12.5px] text-muted space-y-2">
            <p>Windows does not expose CPU, motherboard or fan sensors to normal programs — reading them needs a kernel driver and administrator rights. PULSE does not bundle a driver.</p>
            <p>
              To enable them, run <b className="text-ink">LibreHardwareMonitor</b> as administrator and turn on <b className="text-ink">Options → Remote Web Server → Run</b> (loopback port 8085). PULSE detects it automatically (checked every 20 s); CPU temperature, per-core temperatures, fan speeds and package power then appear here and on the Overview. Nothing leaves your PC.
            </p>
            {lhm?.installed ? (
              <div className="flex items-center gap-3 pt-1">
                <Btn onClick={() => api.lhmLaunch().catch((e) => toast({ kind: "bad", title: "Could not start LibreHardwareMonitor", body: String(e) }))}>Start LibreHardwareMonitor (asks for administrator)</Btn>
                <span className="text-[11.5px] text-faint selectable">{lhm.path}</span>
              </div>
            ) : (
              <p className="text-faint">PULSE does not download anything. Place LibreHardwareMonitor in <span className="selectable">%APPDATA%\PULSE\tools\LibreHardwareMonitor</span> to get a launch button here.</p>
            )}
            <p className="text-faint">Status: {hub?.status}</p>
          </div>
        )}
        {s.net && null}
      </Card>

      <SensorStatus snap={s} inv={inv} hub={hub} />
    </div>
  );
}

function SensorList({ title, items, empty }: { title: string; items: { n: string; v: string }[]; empty?: string }) {
  return (
    <div>
      <div className="text-[11px] uppercase tracking-wide text-muted mb-1">{title}</div>
      {items.length === 0 ? <div className="text-[12px] text-faint">{empty ?? "None."}</div> : items.map((i, k) => <div key={k} className="flex justify-between text-[12.5px] py-0.5"><span className="text-muted truncate pr-2">{i.n}</span><span className="num">{i.v}</span></div>)}
    </div>
  );
}

function SensorStatus({ snap, inv, hub }: { snap: ReturnType<typeof useStore.getState>["snap"] & {}; inv: Inventory | null; hub: HubData | null }) {
  const g = snap.gpu;
  const st = (r?: Reading | null): { state: string; tone: "good" | "warn" | "neutral" | "bad"; note: string } => {
    if (!r) return { state: "Unavailable", tone: "neutral", note: "" };
    if (r.value != null) return { state: "Available", tone: "good", note: r.note ?? "" };
    if (r.status === "needs_admin") return { state: "Requires elevation / driver", tone: "warn", note: r.note ?? "" };
    if (r.status === "unsupported") return { state: "Not supported", tone: "neutral", note: r.note ?? "" };
    return { state: "Temporarily unavailable", tone: "neutral", note: r.note ?? "" };
  };
  const ok = (note: string) => ({ state: "Available", tone: "good" as const, note });
  const rows: { m: string; src: string; s: { state: string; tone: any; note: string } }[] = [
    { m: "CPU utilization (total / per core)", src: "Windows kernel counters", s: ok("") },
    { m: "CPU frequency", src: "Windows PDH", s: snap.cpu.freqMhz != null ? ok("") : { state: "Unavailable", tone: "neutral", note: "Counter missing" } },
    { m: "CPU temperature", src: hub?.status === "ok" ? hub.source : "Sensor driver", s: st(snap.cpuTemp) },
    { m: "CPU power", src: hub?.status === "ok" ? hub.source : "Sensor driver", s: st(snap.cpuPower) },
    { m: "RAM, commit, pagefile, kernel pools", src: "Windows memory APIs", s: ok("") },
    { m: "Standby / modified / free lists", src: "Windows PDH", s: snap.mem.standby != null ? ok("") : { state: "Unavailable", tone: "neutral", note: "Counters missing" } },
    { m: "GPU utilization", src: "NVML", s: st(g?.util) },
    { m: "GPU temperature", src: "NVML", s: st(g?.temp) },
    { m: "GPU hotspot temperature", src: "NVML", s: st(g?.hotspot) },
    { m: "GPU fan speed (%)", src: "NVML", s: st(g?.fanPct) },
    { m: "GPU fan speed (RPM)", src: "NVML", s: st(g?.fanRpm) },
    { m: "GPU power draw / limit", src: "NVML", s: st(g?.powerW) },
    { m: "GPU clocks", src: "NVML", s: st(g?.clockGpu) },
    { m: "VRAM", src: "NVML", s: st(g?.vramUsed) },
    { m: "Per-process GPU % and VRAM", src: "Windows GPU counters", s: ok("Per-process GPU usage and dedicated VRAM") },
    { m: "Disk throughput, activity, latency", src: "Windows PDH", s: snap.diskIo.readBps != null ? ok("") : { state: "Unavailable", tone: "neutral", note: "" } },
    { m: "Drive temperature / SMART", src: "Windows Storage", s: inv?.disks.some((d) => d.temperature != null) ? ok("") : { state: "Requires elevation / not reported", tone: "warn", note: "Drive did not report SMART data to Windows for a standard user" } },
    { m: "Motherboard & chassis fans", src: hub?.status === "ok" ? hub.source : "Sensor driver", s: hub?.status === "ok" ? (hub.sensors.some((x) => x.kind === "Fan") ? ok("") : { state: "Not supported", tone: "neutral", note: "None exposed by board" }) : { state: "Requires elevation / driver", tone: "warn", note: "Run LibreHardwareMonitor with its web server enabled" } },
    { m: "Per-process network usage", src: "—", s: { state: "Not supported", tone: "neutral", note: "Needs elevated ETW tracing; not collected" } },
  ];
  return (
    <Card title="Hardware sensor status" tip="One place to see which readings are available on this machine and why the others are not." pad={false}>
      <table className="w-full text-[12.5px]">
        <thead>
          <tr className="text-left text-muted text-[11px] uppercase tracking-wide">
            <th className="px-4 py-2">Metric</th>
            <th className="px-3 py-2">Source</th>
            <th className="px-3 py-2">State</th>
            <th className="px-3 py-2">Note</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.m} className="border-t border-line/60">
              <td className="px-4 py-1.5">{r.m}</td>
              <td className="px-3 py-1.5 text-muted">{r.src}</td>
              <td className="px-3 py-1.5"><Badge tone={r.s.tone}>{r.s.state}</Badge></td>
              <td className="px-3 py-1.5 text-faint">{r.s.note}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </Card>
  );
}
