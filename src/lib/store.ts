import { create } from "zustand";
import type { ProcRow, ScanMeta, ScanProgress, Settings, SystemSnap, TopApp } from "./types";

export type View = "overview" | "storage" | "processes" | "hardware" | "history" | "settings";

export interface LivePoint {
  t: number;
  cpu: number;
  ramPct: number;
  ramUsed: number;
  gpu: number | null;
  vram: number | null;
  vramPct: number | null;
  gpuTemp: number | null;
  diskR: number | null;
  diskW: number | null;
  netRx: number | null;
  netTx: number | null;
}

export interface Toast {
  id: number;
  kind: "info" | "good" | "warn" | "bad";
  title: string;
  body?: string;
}

interface State {
  view: View;
  navigate: (v: View, opts?: { tab?: string }) => void;
  viewOpts: { tab?: string };
  snap: SystemSnap | null;
  live: LivePoint[];
  procs: ProcRow[];
  topApps: TopApp[];
  setTopApps: (t: TopApp[]) => void;
  settings: Settings | null;
  paused: boolean;
  isAdmin: boolean;
  toasts: Toast[];
  toast: (t: Omit<Toast, "id">) => void;
  dismissToast: (id: number) => void;
  setSnap: (s: SystemSnap) => void;
  setProcs: (p: ProcRow[]) => void;
  setSettings: (s: Settings) => void;
  setPaused: (p: boolean) => void;
  setAdmin: (b: boolean) => void;
  // storage scan state
  scanning: boolean;
  scanProgress: ScanProgress | null;
  scanSaving: boolean;
  scanBatch: { index: number; total: number } | null;
  scanMeta: ScanMeta | null;
  setScan: (p: Partial<Pick<State, "scanning" | "scanProgress" | "scanSaving" | "scanBatch" | "scanMeta">>) => void;
  scanVersion: number;
}

const LIVE_MAX = 1800;
let toastId = 1;

export const useStore = create<State>((set, get) => ({
  view: "overview",
  viewOpts: {},
  navigate: (v, opts) => set({ view: v, viewOpts: opts ?? {} }),
  snap: null,
  live: [],
  procs: [],
  topApps: [],
  setTopApps: (topApps) => set({ topApps }),
  settings: null,
  paused: false,
  isAdmin: false,
  toasts: [],
  toast: (t) => {
    const id = toastId++;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { ...t, id }] }));
    setTimeout(() => get().dismissToast(id), t.kind === "bad" ? 9000 : 5000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) })),
  setSnap: (snap) =>
    set((s) => {
      const g = snap.gpu;
      const vu = g?.vramUsed.value ?? null;
      const vt = g?.vramTotal.value ?? null;
      const p: LivePoint = {
        t: snap.ts,
        cpu: snap.cpu.total,
        ramPct: snap.mem.total ? ((snap.mem.total - snap.mem.available) * 100) / snap.mem.total : 0,
        ramUsed: snap.mem.total - snap.mem.available,
        gpu: g?.util.value ?? null,
        vram: vu,
        vramPct: vu != null && vt ? (vu * 100) / vt : null,
        gpuTemp: g?.temp.value ?? null,
        diskR: snap.diskIo.readBps,
        diskW: snap.diskIo.writeBps,
        netRx: snap.net.rxBps,
        netTx: snap.net.txBps,
      };
      const live = s.live.length >= LIVE_MAX ? [...s.live.slice(s.live.length - LIVE_MAX + 1), p] : [...s.live, p];
      return { snap, live };
    }),
  setProcs: (procs) => set({ procs }),
  setSettings: (settings) => set({ settings }),
  setPaused: (paused) => set({ paused }),
  setAdmin: (isAdmin) => set({ isAdmin }),
  scanning: false,
  scanProgress: null,
  scanSaving: false,
  scanBatch: null,
  scanMeta: null,
  setScan: (p) => set((s) => ({ ...p, scanVersion: p.scanMeta !== undefined && p.scanMeta !== s.scanMeta ? s.scanVersion + 1 : s.scanVersion })),
  scanVersion: 0,
}));
