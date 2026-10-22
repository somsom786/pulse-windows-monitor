import { ReactNode, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export function cx(...a: (string | false | null | undefined)[]) {
  return a.filter(Boolean).join(" ");
}

export function Card({ title, right, children, className, pad = true, tip }: { title?: ReactNode; right?: ReactNode; children: ReactNode; className?: string; pad?: boolean; tip?: string }) {
  return (
    <section className={cx("rounded-xl border border-line bg-surface min-w-0", className)}>
      {(title || right) && (
        <header className="flex items-center justify-between gap-3 px-4 pt-3 pb-1">
          <h3 className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted flex items-center gap-1.5">
            {title}
            {tip && <Info text={tip} />}
          </h3>
          {right}
        </header>
      )}
      <div className={pad ? "p-4 pt-2" : ""}>{children}</div>
    </section>
  );
}

export function Info({ text }: { text: string }) {
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  return (
    <span
      className="inline-flex items-center justify-center w-3.5 h-3.5 rounded-full border border-line2 text-[9px] text-faint cursor-help normal-case tracking-normal font-medium hover:text-ink hover:border-muted"
      onMouseEnter={(e) => {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        setPos({ x: r.left + r.width / 2, y: r.bottom + 6 });
      }}
      onMouseLeave={() => setPos(null)}
    >
      ?
      {pos &&
        createPortal(
          <div className="fixed z-[100] max-w-[280px] rounded-lg border border-line2 bg-surface3 px-3 py-2 text-[12px] leading-snug text-ink shadow-xl normal-case tracking-normal font-normal pointer-events-none fade-in" style={{ left: Math.min(pos.x - 20, window.innerWidth - 300), top: pos.y }}>
            {text}
          </div>,
          document.body,
        )}
    </span>
  );
}

export function Btn({ children, onClick, kind = "default", disabled, title, className, small }: { children: ReactNode; onClick?: (e: React.MouseEvent) => void; kind?: "default" | "primary" | "danger" | "ghost"; disabled?: boolean; title?: string; className?: string; small?: boolean }) {
  const base = "inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors select-none disabled:opacity-40 disabled:pointer-events-none whitespace-nowrap";
  const size = small ? "px-2.5 py-1 text-[12px]" : "px-3.5 py-1.5 text-[13px]";
  const k = {
    default: "bg-surface3 hover:bg-line text-ink border border-line",
    primary: "bg-accent text-[#04141c] hover:brightness-110",
    danger: "bg-bad/15 text-bad hover:bg-bad/25 border border-bad/30",
    ghost: "text-muted hover:text-ink hover:bg-surface3",
  }[kind];
  return (
    <button className={cx(base, size, k, className)} onClick={onClick} disabled={disabled} title={title}>
      {children}
    </button>
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "good" | "warn" | "bad" | "accent" }) {
  const t = {
    neutral: "bg-surface3 text-muted",
    good: "bg-good/15 text-good",
    warn: "bg-warn/15 text-warn",
    bad: "bg-bad/15 text-bad",
    accent: "bg-accentsoft text-accent",
  }[tone];
  return <span className={cx("inline-flex items-center rounded-md px-1.5 py-0.5 text-[11px] font-medium", t)}>{children}</span>;
}

export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: ReactNode; title?: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="inline-flex rounded-lg bg-surface2 border border-line p-0.5">
      {options.map((o) => (
        <button key={o.value} title={o.title} onClick={() => onChange(o.value)} className={cx("px-2.5 py-1 rounded-md text-[12px] font-medium transition-colors", value === o.value ? "bg-surface3 text-ink shadow-sm" : "text-muted hover:text-ink")}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Tabs<T extends string>({ value, tabs, onChange }: { value: T; tabs: { id: T; label: string; badge?: ReactNode }[]; onChange: (v: T) => void }) {
  return (
    <div className="flex gap-1 border-b border-line overflow-x-auto">
      {tabs.map((t) => (
        <button key={t.id} onClick={() => onChange(t.id)} className={cx("relative px-3 py-2 text-[13px] font-medium whitespace-nowrap transition-colors flex items-center gap-1.5", value === t.id ? "text-ink" : "text-muted hover:text-ink")}>
          {t.label}
          {t.badge}
          {value === t.id && <span className="absolute left-2 right-2 -bottom-px h-[2px] rounded bg-accent" />}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ checked, onChange, label, hint }: { checked: boolean; onChange: (v: boolean) => void; label?: ReactNode; hint?: ReactNode }) {
  return (
    <label className="flex items-start justify-between gap-4 cursor-pointer py-1.5">
      <span className="min-w-0">
        {label && <span className="block text-[13px]">{label}</span>}
        {hint && <span className="block text-[12px] text-muted">{hint}</span>}
      </span>
      <button type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)} className={cx("relative shrink-0 mt-0.5 w-9 h-5 rounded-full transition-colors", checked ? "bg-accent" : "bg-line2")}>
        <span className={cx("absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform", checked && "translate-x-4")} />
      </button>
    </label>
  );
}

export function Bar({ value, max = 100, tone, height = 6, className }: { value: number; max?: number; tone?: "auto" | "accent" | "good" | "warn" | "bad" | string; height?: number; className?: string }) {
  const p = Math.max(0, Math.min(100, (value / max) * 100));
  let color = "var(--accent)";
  if (tone === "auto") color = p > 90 ? "var(--bad)" : p > 75 ? "var(--warn)" : "var(--accent)";
  else if (tone === "good") color = "var(--good)";
  else if (tone === "warn") color = "var(--warn)";
  else if (tone === "bad") color = "var(--bad)";
  else if (tone && tone !== "accent") color = tone;
  return (
    <div className={cx("w-full rounded-full bg-surface3 overflow-hidden", className)} style={{ height }}>
      <div className="h-full rounded-full transition-[width] duration-500 ease-out" style={{ width: `${p}%`, background: color }} />
    </div>
  );
}

export function Empty({ icon, title, body, action }: { icon?: ReactNode; title: string; body?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center gap-2 py-12 px-6">
      {icon && <div className="text-faint mb-1">{icon}</div>}
      <div className="text-[14px] font-medium">{title}</div>
      {body && <div className="text-[12.5px] text-muted max-w-md">{body}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Spinner({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" className="animate-spin text-accent">
      <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeOpacity="0.2" strokeWidth="3" />
      <path d="M21 12a9 9 0 0 0-9-9" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
    </svg>
  );
}

export function Modal({ open, title, children, onClose, footer, width = 460 }: { open: boolean; title: string; children: ReactNode; onClose: () => void; footer?: ReactNode; width?: number }) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);
  if (!open) return null;
  return createPortal(
    <div className="fixed inset-0 z-[90] flex items-center justify-center bg-black/55 fade-in" onMouseDown={onClose}>
      <div className="rounded-xl border border-line2 bg-surface shadow-2xl max-h-[85vh] flex flex-col" style={{ width, maxWidth: "92vw" }} onMouseDown={(e) => e.stopPropagation()}>
        <div className="px-5 pt-4 pb-2 text-[15px] font-semibold">{title}</div>
        <div className="px-5 pb-4 text-[13px] text-muted overflow-auto selectable">{children}</div>
        {footer && <div className="px-5 py-3 border-t border-line flex justify-end gap-2">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export function useConfirm() {
  const [state, setState] = useState<{ title: string; body: ReactNode; confirm: string; danger?: boolean; resolve: (v: boolean) => void } | null>(null);
  const ask = (title: string, body: ReactNode, confirm = "Confirm", danger = false) => new Promise<boolean>((resolve) => setState({ title, body, confirm, danger, resolve }));
  const close = (v: boolean) => {
    state?.resolve(v);
    setState(null);
  };
  const node = (
    <Modal
      open={!!state}
      title={state?.title ?? ""}
      onClose={() => close(false)}
      footer={
        <>
          <Btn onClick={() => close(false)}>Cancel</Btn>
          <Btn kind={state?.danger ? "danger" : "primary"} onClick={() => close(true)}>
            {state?.confirm}
          </Btn>
        </>
      }
    >
      {state?.body}
    </Modal>
  );
  return { ask, node };
}

export function Stat({ label, value, sub, tone, tip }: { label: ReactNode; value: ReactNode; sub?: ReactNode; tone?: "good" | "warn" | "bad"; tip?: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[11px] text-muted flex items-center gap-1">
        {label}
        {tip && <Info text={tip} />}
      </div>
      <div className={cx("text-[15px] font-semibold num truncate", tone === "good" && "text-good", tone === "warn" && "text-warn", tone === "bad" && "text-bad")}>{value}</div>
      {sub && <div className="text-[11px] text-faint num truncate">{sub}</div>}
    </div>
  );
}

export function ReadingView({ r, fmt, label }: { r: { value: number | null; status: string; note: string | null } | null | undefined; fmt: (n: number) => string; label?: string }) {
  if (!r) return <span className="text-faint">—</span>;
  if (r.value != null) return <span className="num">{fmt(r.value)}</span>;
  const text = r.status === "needs_admin" ? "Needs elevation" : r.status === "unsupported" ? "Not supported" : "Unavailable";
  const tone = r.status === "needs_admin" ? "warn" : "neutral";
  return (
    <span title={r.note ?? undefined} className="inline-flex items-center gap-1 cursor-help">
      <Badge tone={tone as any}>{text}</Badge>
      {label}
    </span>
  );
}

export function useInterval(fn: () => void, ms: number | null, deps: any[] = []) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    if (ms == null) return;
    const id = setInterval(() => ref.current(), ms);
    return () => clearInterval(id);
  }, [ms, ...deps]);
}

export function useDebounced<T>(v: T, ms = 250): T {
  const [d, setD] = useState(v);
  useEffect(() => {
    const t = setTimeout(() => setD(v), ms);
    return () => clearTimeout(t);
  }, [v, ms]);
  return d;
}

export const Icon = {
  overview: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <rect x="3" y="3" width="7.5" height="9" rx="1.6" />
      <rect x="13.5" y="3" width="7.5" height="5" rx="1.6" />
      <rect x="13.5" y="11" width="7.5" height="10" rx="1.6" />
      <rect x="3" y="15" width="7.5" height="6" rx="1.6" />
    </svg>
  ),
  storage: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <rect x="3" y="3" width="18" height="18" rx="2.5" />
      <path d="M3 12h11M14 3v18M14 9h7M9 12v9" />
    </svg>
  ),
  memory: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <rect x="2.5" y="7" width="19" height="9" rx="1.5" />
      <path d="M6 16v3M10 16v3M14 16v3M18 16v3M6.5 10.5h3v2h-3zM14.5 10.5h3v2h-3z" />
    </svg>
  ),
  hardware: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <rect x="6" y="6" width="12" height="12" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" rx="0.8" />
      <path d="M9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3" />
    </svg>
  ),
  history: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5M12 7.5V12l3 2" />
    </svg>
  ),
  settings: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  ),
  folder: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" {...p}>
      <path d="M3 6.5A2.5 2.5 0 0 1 5.5 4h4l2 2.5h7A2.5 2.5 0 0 1 21 9v8.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z" />
    </svg>
  ),
  file: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" {...p}>
      <path d="M6 3h8l5 5v12a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
      <path d="M14 3v5h5" />
    </svg>
  ),
  drive: (p: any) => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M5.5 13 8 5h8l2.5 8M17 16.5h.01" />
    </svg>
  ),
  chevron: (p: any) => (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <path d="m9 6 6 6-6 6" />
    </svg>
  ),
  search: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" {...p}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  ),
  pause: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" {...p}>
      <rect x="6" y="5" width="4" height="14" rx="1" />
      <rect x="14" y="5" width="4" height="14" rx="1" />
    </svg>
  ),
  play: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" {...p}>
      <path d="M7 4.5v15l12-7.5z" />
    </svg>
  ),
  more: (p: any) => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" {...p}>
      <circle cx="5" cy="12" r="1.8" />
      <circle cx="12" cy="12" r="1.8" />
      <circle cx="19" cy="12" r="1.8" />
    </svg>
  ),
  warn: (p: any) => (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" {...p}>
      <path d="M12 3 2 20h20zM12 10v4.5M12 17.5v.01" />
    </svg>
  ),
};
