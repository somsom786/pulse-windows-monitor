export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n)) return "—";
  const a = Math.abs(n);
  if (a >= 1099511627776) return (n / 1099511627776).toFixed(2) + " TB";
  if (a >= 1073741824) return (n / 1073741824).toFixed(digits) + " GB";
  if (a >= 1048576) return (n / 1048576).toFixed(a >= 104857600 ? 0 : digits) + " MB";
  if (a >= 1024) return (n / 1024).toFixed(0) + " KB";
  return Math.round(n) + " B";
}

export function rate(n: number | null | undefined): string {
  if (n == null || !isFinite(n)) return "—";
  return bytes(n, 1) + "/s";
}

export function bits(n: number | null | undefined): string {
  if (n == null || !isFinite(n)) return "—";
  const b = n * 8;
  if (b >= 1e9) return (b / 1e9).toFixed(2) + " Gbps";
  if (b >= 1e6) return (b / 1e6).toFixed(1) + " Mbps";
  if (b >= 1e3) return (b / 1e3).toFixed(0) + " Kbps";
  return Math.round(b) + " bps";
}

export function pct(n: number | null | undefined, digits = 0): string {
  if (n == null || !isFinite(n)) return "—";
  return n.toFixed(digits) + "%";
}

export function num(n: number | null | undefined, digits = 0): string {
  if (n == null || !isFinite(n)) return "—";
  return n.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

export function mhz(n: number | null | undefined): string {
  if (n == null) return "—";
  return n >= 1000 ? (n / 1000).toFixed(2) + " GHz" : Math.round(n) + " MHz";
}

export function temp(c: number | null | undefined, unit: "C" | "F" = "C"): string {
  if (c == null || !isFinite(c)) return "—";
  return unit === "F" ? Math.round(c * 1.8 + 32) + "°F" : Math.round(c) + "°C";
}

export function dur(s: number): string {
  const d = Math.floor(s / 86400),
    h = Math.floor((s % 86400) / 3600),
    m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${Math.floor(s % 60)}s`;
  return `${Math.floor(s)}s`;
}

export function ms(n: number): string {
  return n < 1000 ? `${Math.round(n)} ms` : dur(n / 1000);
}

export function timeOfDay(ts: number, seconds = false): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: seconds ? "2-digit" : undefined });
}

export function dateTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleDateString([], { month: "short", day: "numeric" }) + " " + timeOfDay(ts, true);
}

export function ago(unixSecs: number): string {
  const s = Math.max(0, Date.now() / 1000 - unixSecs);
  if (s < 90) return "just now";
  if (s < 5400) return Math.round(s / 60) + " min ago";
  if (s < 129600) return Math.round(s / 3600) + " h ago";
  return Math.round(s / 86400) + " d ago";
}

export function fileTimeToMs(ft: number): number {
  return ft / 10000 - 11644473600000;
}

export function fmtDate(unixSecs: number): string {
  if (!unixSecs) return "—";
  return new Date(unixSecs * 1000).toLocaleDateString([], { year: "numeric", month: "short", day: "numeric" });
}

export const CAT_NAMES = ["Other", "Video", "Audio", "Images", "Archives", "Documents", "Code & projects", "Programs & libraries", "Disk images & VMs", "Game & engine data", "Databases & caches", "System & logs"];

// Category palette: muted, distinguishable on dark and light
export const CAT_COLORS = ["#6b7488", "#e0707a", "#c58bf0", "#4fb8a1", "#e5a64b", "#6aa0f0", "#8fcf6a", "#f0805a", "#4cc1d9", "#d6b84a", "#b08968", "#8a93b0"];
export const FOLDER_COLOR = "#3b475c";
