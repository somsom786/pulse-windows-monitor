import React, { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./lib/api";
import { useStore, View } from "./lib/store";
import type { ProcRow, ScanMeta, ScanProgress, SystemSnap, TopApp } from "./lib/types";
import { Icon, cx } from "./components/ui";
import { Overview } from "./pages/Overview";
import { Storage } from "./pages/Storage";
import { Processes } from "./pages/Processes";
import { Hardware } from "./pages/Hardware";
import { History } from "./pages/History";
import { SettingsPage } from "./pages/Settings";
import { bytes } from "./lib/format";

const NAV: { id: View; label: string; icon: (p: any) => React.JSX.Element }[] = [
  { id: "overview", label: "Overview", icon: Icon.overview },
  { id: "storage", label: "Storage Explorer", icon: Icon.storage },
  { id: "processes", label: "Processes & Memory", icon: Icon.memory },
  { id: "hardware", label: "Hardware", icon: Icon.hardware },
  { id: "history", label: "History", icon: Icon.history },
  { id: "settings", label: "Settings", icon: Icon.settings },
];

export default function App() {
  const { view, navigate, snap, paused, setPaused, scanning, scanProgress, toasts, dismissToast, settings } = useStore();

  useEffect(() => {
    const s = useStore.getState();
    api.getSettings().then(s.setSettings);
    api.getPaused().then(s.setPaused);
    api.appInfo().then((i) => s.setAdmin(i.isAdmin));
    api.getSnapshot().then((x) => x && s.setSnap(x));
    api.scanState().then((st) => st.scanning && s.setScan({ scanning: true }));
    const subs = [
      listen<SystemSnap>("telemetry", (e) => useStore.getState().setSnap(e.payload)),
      listen<ProcRow[]>("processes", (e) => useStore.getState().setProcs(e.payload)),
      listen<TopApp[]>("top-apps", (e) => useStore.getState().setTopApps(e.payload)),
      listen<boolean>("paused", (e) => useStore.getState().setPaused(e.payload)),
      listen<ScanProgress>("scan-progress", (e) => useStore.getState().setScan({ scanning: true, scanProgress: e.payload })),
      listen<{ index: number; total: number }>("scan-start", (e) => useStore.getState().setScan({ scanning: true, scanBatch: e.payload, scanSaving: false })),
      listen<ScanMeta>("scan-saving", () => useStore.getState().setScan({ scanSaving: true })),
      listen<{ meta: ScanMeta; cancelled: boolean }>("scan-done", (e) => {
        const st = useStore.getState();
        if (e.payload.cancelled) st.toast({ kind: "warn", title: "Scan cancelled", body: "The partial result was discarded." });
        else {
          st.setScan({ scanMeta: e.payload.meta });
          st.toast({ kind: "good", title: `Scan complete: ${e.payload.meta.root}`, body: `${e.payload.meta.files.toLocaleString()} files · ${bytes(e.payload.meta.totalAlloc)}` });
        }
      }),
      listen("scan-finished", () => useStore.getState().setScan({ scanning: false, scanSaving: false, scanBatch: null })),
      listen<string>("scan-error", (e) => useStore.getState().toast({ kind: "bad", title: "Scan problem", body: e.payload })),
      listen<{ title: string; body: string }>("alert", (e) => useStore.getState().toast({ kind: "warn", title: e.payload.title, body: e.payload.body })),
    ];
    return () => {
      subs.forEach((p) => p.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    api.setView(view).catch(() => {});
  }, [view]);

  // theme
  useEffect(() => {
    const t = settings?.theme ?? "dark";
    const apply = () => {
      const light = t === "light" || (t === "system" && window.matchMedia("(prefers-color-scheme: light)").matches);
      document.documentElement.dataset.theme = light ? "light" : "dark";
    };
    apply();
    if (t === "system") {
      const mq = window.matchMedia("(prefers-color-scheme: light)");
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [settings?.theme]);

  const stale = snap && Date.now() - snap.ts > (settings?.pollIntervalMs ?? 2000) * 4 + 2000;

  return (
    <div className="h-full flex">
      <aside className="w-[212px] shrink-0 border-r border-line bg-surface flex flex-col">
        <div className="px-5 pt-5 pb-4 flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-lg bg-accent/15 flex items-center justify-center">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
              <path d="M2 12h5l2-7 4 14 3-9 1.5 2H22" />
            </svg>
          </div>
          <div>
            <div className="font-semibold tracking-[0.14em] text-[13px]">PULSE</div>
            <div className="text-[10.5px] text-faint -mt-0.5">System intelligence</div>
          </div>
        </div>
        <nav className="px-3 space-y-0.5 flex-1">
          {NAV.map((n) => (
            <button key={n.id} onClick={() => navigate(n.id)} className={cx("w-full flex items-center gap-3 px-3 py-2 rounded-lg text-[13px] font-medium transition-colors", view === n.id ? "bg-accentsoft text-accent" : "text-muted hover:text-ink hover:bg-surface3")}>
              <n.icon />
              {n.label}
              {n.id === "storage" && scanning && <span className="ml-auto w-1.5 h-1.5 rounded-full bg-accent live-dot" />}
            </button>
          ))}
        </nav>
        <div className="p-3 border-t border-line space-y-2">
          <button
            onClick={() => {
              api.setPaused(!paused);
            }}
            className={cx("w-full flex items-center justify-between rounded-lg px-3 py-2 text-[12px] border transition-colors", paused ? "border-warn/40 bg-warn/10 text-warn" : "border-line text-muted hover:text-ink hover:bg-surface3")}
            title={paused ? "Resume live monitoring and recording" : "Pause live monitoring and recording"}
          >
            <span className="flex items-center gap-2">
              {paused ? <Icon.play /> : <Icon.pause />}
              {paused ? "Monitoring paused" : "Monitoring live"}
            </span>
            {!paused && <span className={cx("w-1.5 h-1.5 rounded-full", stale ? "bg-warn" : "bg-good live-dot")} />}
          </button>
          <div className="text-[10.5px] text-faint px-1 num">{snap ? `Sampled in ${snap.sampleCostMs.toFixed(0)} ms · every ${(snap.pollMs / 1000).toFixed(snap.pollMs % 1000 ? 1 : 0)} s` : "Starting…"}</div>
        </div>
      </aside>
      <main className="flex-1 min-w-0 h-full relative">
        <div className="h-full" key={view}>
          <div className="h-full fade-in">
            {view === "overview" && <Overview />}
            {view === "storage" && <Storage />}
            {view === "processes" && <Processes />}
            {view === "hardware" && <Hardware />}
            {view === "history" && <History />}
            {view === "settings" && <SettingsPage />}
          </div>
        </div>
        <div className="absolute right-4 bottom-4 flex flex-col gap-2 z-[80] w-[340px] max-w-[90vw]">
          {toasts.map((t) => (
            <div key={t.id} className="fade-in rounded-xl border border-line2 bg-surface3 shadow-2xl px-4 py-3 flex gap-3 items-start" onClick={() => dismissToast(t.id)}>
              <span className={cx("mt-1 w-2 h-2 rounded-full shrink-0", t.kind === "good" && "bg-good", t.kind === "warn" && "bg-warn", t.kind === "bad" && "bg-bad", t.kind === "info" && "bg-accent")} />
              <div className="min-w-0">
                <div className="text-[13px] font-medium">{t.title}</div>
                {t.body && <div className="text-[12px] text-muted break-words">{t.body}</div>}
              </div>
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}
