import { ReactNode, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import type { AppInfo, Settings, StoreInfo } from "../lib/types";
import { bytes, dateTime, num } from "../lib/format";
import { Badge, Btn, Card, Segmented, Toggle, useConfirm } from "../components/ui";

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-6 py-2">
      <div className="min-w-0">
        <div className="text-[13px]">{label}</div>
        {hint && <div className="text-[12px] text-muted max-w-md">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Num({ value, onChange, min, max, step = 1, unit }: { value: number; onChange: (v: number) => void; min: number; max: number; step?: number; unit?: string }) {
  const [txt, setTxt] = useState(String(value));
  useEffect(() => setTxt(String(value)), [value]);
  return (
    <span className="inline-flex items-center gap-1.5">
      <input
        type="number"
        className="w-24 text-right"
        value={txt}
        min={min}
        max={max}
        step={step}
        onChange={(e) => setTxt(e.target.value)}
        onBlur={() => {
          const v = Math.min(max, Math.max(min, parseFloat(txt) || min));
          setTxt(String(v));
          if (v !== value) onChange(v);
        }}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
      {unit && <span className="text-muted text-[12px] w-14 whitespace-nowrap">{unit}</span>}
    </span>
  );
}

export function SettingsPage() {
  const { settings, setSettings, isAdmin, toast } = useStore();
  const [info, setInfo] = useState<StoreInfo | null>(null);
  const [app, setApp] = useState<AppInfo | null>(null);
  const [auto, setAuto] = useState(false);
  const { ask, node } = useConfirm();

  const refresh = () => api.storeInfo().then(setInfo);
  useEffect(() => {
    refresh();
    api.appInfo().then(setApp);
    api.autostartGet().then(setAuto).catch(() => {});
  }, []);
  if (!settings) return null;

  const set = async (patch: Partial<Settings>) => {
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSettings(await api.setSettings(next));
    } catch (e) {
      toast({ kind: "bad", title: "Could not save settings", body: String(e) });
    }
  };

  const approxRows = (settings.retentionDays * 86400) / settings.recordIntervalS;
  const approxSnap = ((settings.retentionDays * 86400) / settings.procSnapshotIntervalS) * 22;
  const approxMb = (approxRows * 90 + approxSnap * 70) / 1048576;

  return (
    <div className="h-full overflow-auto p-5">
      {node}
      <div className="max-w-3xl space-y-4">
        <div>
          <h1 className="text-[20px] font-semibold tracking-tight">Settings</h1>
          <p className="text-muted text-[12.5px] mt-0.5">Changes save immediately. Everything stays on this PC.</p>
        </div>

        <Card title="Monitoring">
          <Field label="Live refresh interval" hint="How often live readings are sampled. 2 seconds keeps PULSE's own CPU use near zero; faster costs more.">
            <Segmented
              value={String(settings.pollIntervalMs)}
              onChange={(v) => set({ pollIntervalMs: parseInt(v) })}
              options={[
                { value: "1000", label: "1 s" },
                { value: "2000", label: "2 s" },
                { value: "5000", label: "5 s" },
                { value: "10000", label: "10 s" },
              ]}
            />
          </Field>
          <Field label="Pause monitoring" hint="Stops sampling and recording entirely until resumed.">
            <Btn onClick={() => api.setPaused(!useStore.getState().paused)}>{useStore.getState().paused ? "Resume" : "Pause"}</Btn>
          </Field>
        </Card>

        <Card title="History recording">
          <Toggle checked={settings.recordEnabled} onChange={(v) => set({ recordEnabled: v })} label="Record history" hint="Stores resource samples and process snapshots in a local SQLite database so you can investigate spikes later. PULSE cannot show anything from before recording started." />
          <Field label="Sample interval" hint="One row of system metrics is stored every N seconds (values averaged over the interval).">
            <Num value={settings.recordIntervalS} onChange={(v) => set({ recordIntervalS: v })} min={2} max={300} unit="sec" />
          </Field>
          <Field label="Process snapshot interval" hint="How often the top processes are recorded. Shorter intervals give finer spike attribution but a larger database.">
            <Num value={settings.procSnapshotIntervalS} onChange={(v) => set({ procSnapshotIntervalS: v })} min={5} max={600} unit="sec" />
          </Field>
          <Field label="Keep history for" hint={`Older data is pruned hourly. Estimated size at these settings: about ${approxMb < 1 ? "<1" : approxMb.toFixed(0)} MB.`}>
            <Num value={settings.retentionDays} onChange={(v) => set({ retentionDays: v })} min={1} max={90} unit="days" />
          </Field>
          <div className="mt-2 rounded-lg bg-surface2 border border-line px-3 py-2 text-[12.5px] flex items-center justify-between gap-3 flex-wrap">
            <span className="text-muted num">
              {info ? `${num(info.samples)} samples · ${num(info.snapshots)} process rows · ${num(info.spikes)} spikes · ${bytes(info.sizeBytes)}${info.oldestTs ? ` · since ${dateTime(info.oldestTs)}` : ""}` : "…"}
            </span>
            <Btn
              small
              kind="danger"
              onClick={async () => {
                if (await ask("Clear all recorded history?", "This deletes all recorded samples, process snapshots, spikes and alerts. Saved storage scans are not affected.", "Clear history", true)) {
                  await api.clearHistory();
                  refresh();
                  toast({ kind: "good", title: "History cleared" });
                }
              }}
            >
              Clear history
            </Btn>
          </div>
        </Card>

        <Card title="Alerts & notifications" tip="Alerts are checked on every sample. Each alert type has a cooldown so it cannot spam you, and every alert is also logged on the History page.">
          <Toggle checked={settings.notifications} onChange={(v) => set({ notifications: v })} label="Windows notifications" hint="Show a desktop notification when an alert fires. Alerts always appear inside PULSE." />
          <div className="border-t border-line my-2" />
          <Toggle checked={settings.alertMemEnabled} onChange={(v) => set({ alertMemEnabled: v })} label="Memory pressure" hint="RAM use (total minus available) stays above the threshold." />
          {settings.alertMemEnabled && (
            <div className="pl-4 border-l border-line ml-1">
              <Field label="Threshold"><Num value={settings.alertMemPct} onChange={(v) => set({ alertMemPct: v })} min={50} max={99} unit="% RAM" /></Field>
              <Field label="Sustained for"><Num value={settings.alertMemSustainS} onChange={(v) => set({ alertMemSustainS: v })} min={5} max={3600} unit="sec" /></Field>
            </div>
          )}
          <Field label="Memory spike sensitivity" hint="Log a spike when RAM use rises by this much within 2 minutes.">
            <Num value={settings.spikeMemGb} onChange={(v) => set({ spikeMemGb: v })} min={0.5} max={32} step={0.5} unit="GB" />
          </Field>
          <Toggle checked={settings.alertDiskEnabled} onChange={(v) => set({ alertDiskEnabled: v })} label="Low disk space" hint="A fixed drive has less than the chosen percentage free." />
          {settings.alertDiskEnabled && (
            <div className="pl-4 border-l border-line ml-1">
              <Field label="Free space below"><Num value={settings.alertDiskFreePct} onChange={(v) => set({ alertDiskFreePct: v })} min={1} max={50} unit="%" /></Field>
            </div>
          )}
          <Toggle checked={settings.alertGpuTempEnabled} onChange={(v) => set({ alertGpuTempEnabled: v })} label="GPU temperature" />
          {settings.alertGpuTempEnabled && (
            <div className="pl-4 border-l border-line ml-1">
              <Field label="Above"><Num value={settings.alertGpuTempC} onChange={(v) => set({ alertGpuTempC: v })} min={50} max={110} unit="°C" /></Field>
            </div>
          )}
          <Toggle checked={settings.alertCpuEnabled} onChange={(v) => set({ alertCpuEnabled: v })} label="Sustained high CPU" />
          {settings.alertCpuEnabled && (
            <div className="pl-4 border-l border-line ml-1">
              <Field label="Above"><Num value={settings.alertCpuPct} onChange={(v) => set({ alertCpuPct: v })} min={50} max={100} unit="%" /></Field>
              <Field label="For"><Num value={settings.alertCpuSustainS} onChange={(v) => set({ alertCpuSustainS: v })} min={10} max={3600} unit="sec" /></Field>
            </div>
          )}
          <Field label="Cooldown between repeats of the same alert" hint="Prevents notification spam.">
            <Num value={settings.alertCooldownMin} onChange={(v) => set({ alertCooldownMin: v })} min={1} max={1440} unit="min" />
          </Field>
        </Card>

        <Card title="Desktop">
          <Toggle checked={settings.minimizeToTray} onChange={(v) => set({ minimizeToTray: v })} label="Keep running in the system tray when closed" hint="Closing the window hides PULSE to the tray instead of quitting. Quit from the tray menu." />
          <Toggle checked={settings.monitorInBackground} onChange={(v) => set({ monitorInBackground: v })} label="Keep monitoring while minimized or hidden" hint="Required for history and spike detection to keep recording while the window is closed. If off, sampling pauses whenever the window is hidden." />
          <Toggle
            checked={auto}
            onChange={async (v) => {
              try {
                await api.autostartSet(v);
                setAuto(v);
              } catch (e) {
                toast({ kind: "bad", title: "Could not change startup setting", body: String(e) });
              }
            }}
            label="Launch PULSE when I sign in to Windows"
            hint="Starts minimized to the tray. Uses a per-user startup entry; no service is installed."
          />
        </Card>

        <Card title="Appearance">
          <Field label="Theme">
            <Segmented value={settings.theme} onChange={(v) => set({ theme: v })} options={[{ value: "dark", label: "Dark" }, { value: "light", label: "Light" }, { value: "system", label: "System" }]} />
          </Field>
          <Field label="Temperature unit">
            <Segmented value={settings.tempUnit} onChange={(v) => set({ tempUnit: v })} options={[{ value: "C", label: "°C" }, { value: "F", label: "°F" }]} />
          </Field>
        </Card>

        <Card title="Privileges">
          <div className="flex items-center justify-between gap-4">
            <div className="text-[12.5px] text-muted max-w-xl">
              PULSE runs fine without administrator rights and uses none for monitoring. Running elevated lets the storage scan read protected folders (a more complete drive total) and lets Windows return SMART data for some drives. It is never required.
            </div>
            {isAdmin ? (
              <Badge tone="good">Running as administrator</Badge>
            ) : (
              <Btn
                onClick={async () => {
                  if (await ask("Restart PULSE as administrator?", "Windows will show a permission prompt. PULSE will close and reopen with elevated rights; history and saved scans are kept.", "Restart elevated")) {
                    invoke("restart_elevated").catch((e) => toast({ kind: "bad", title: "Could not elevate", body: String(e) }));
                  }
                }}
              >
                Restart as administrator
              </Btn>
            )}
          </div>
        </Card>

        <Card title="About">
          <div className="text-[12.5px] text-muted space-y-1 selectable">
            <div>
              PULSE {app?.version} · PID {app?.pid}
            </div>
            <div>Data folder: {app?.dataDir}</div>
            <div>No network access is used. No accounts, telemetry or cloud services.</div>
          </div>
        </Card>
      </div>
    </div>
  );
}
