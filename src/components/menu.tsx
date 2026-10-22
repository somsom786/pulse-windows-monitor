import { ReactNode, useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { cx, useConfirm } from "./ui";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { bytes } from "../lib/format";

export interface MenuItem {
  label: string;
  onClick?: () => void;
  danger?: boolean;
  disabled?: boolean;
  sep?: boolean;
  hint?: string;
}

export function useMenu() {
  const [m, setM] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const open = useCallback((e: { clientX: number; clientY: number; preventDefault?: () => void }, items: MenuItem[]) => {
    e.preventDefault?.();
    setM({ x: e.clientX, y: e.clientY, items });
  }, []);
  useEffect(() => {
    if (!m) return;
    const close = () => setM(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", close);
    window.addEventListener("wheel", close, { passive: true });
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", close);
      window.removeEventListener("wheel", close);
    };
  }, [m]);
  const node: ReactNode = m
    ? createPortal(
        <div className="fixed z-[95] min-w-[200px] rounded-lg border border-line2 bg-surface3 py-1 shadow-2xl fade-in" style={{ left: Math.min(m.x, window.innerWidth - 230), top: Math.min(m.y, window.innerHeight - m.items.length * 30 - 16) }} onMouseDown={(e) => e.stopPropagation()}>
          {m.items.map((it, i) =>
            it.sep ? (
              <div key={i} className="my-1 h-px bg-line" />
            ) : (
              <button
                key={i}
                disabled={it.disabled}
                title={it.hint}
                onClick={() => {
                  setM(null);
                  it.onClick?.();
                }}
                className={cx("w-full text-left px-3 py-1.5 text-[12.5px] hover:bg-line disabled:opacity-40 disabled:pointer-events-none", it.danger && "text-bad")}
              >
                {it.label}
              </button>
            ),
          )}
        </div>,
        document.body,
      )
    : null;
  return { open, node };
}

/** Shared file/folder actions with confirmation and safeguards. */
export function useFileActions(onChanged?: () => void) {
  const toast = useStore((s) => s.toast);
  const { ask, node } = useConfirm();

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast({ kind: "good", title: "Path copied", body: text });
    } catch {
      toast({ kind: "bad", title: "Could not copy to clipboard" });
    }
  };
  const run = async (p: Promise<any>, ok?: string) => {
    try {
      await p;
      if (ok) toast({ kind: "good", title: ok });
    } catch (e) {
      toast({ kind: "bad", title: "Action failed", body: String(e) });
    }
  };
  const recycle = async (paths: string[], size?: number) => {
    for (const p of paths) {
      const why = await api.isProtected(p);
      if (why) {
        toast({ kind: "bad", title: "Protected location", body: `${p} is ${why}. PULSE will not remove it.` });
        return;
      }
    }
    const ok = await ask(
      "Move to Recycle Bin?",
      <div className="space-y-2">
        <div>{paths.length === 1 ? "This item will be moved to the Windows Recycle Bin, where you can restore it." : `${paths.length} items will be moved to the Windows Recycle Bin.`}</div>
        <div className="rounded-md bg-surface2 border border-line p-2 text-[12px] text-ink break-all max-h-32 overflow-auto">{paths.join("\n")}</div>
        {size != null && <div>Frees about {bytes(size)} once the Recycle Bin is emptied.</div>}
        <div className="text-faint text-[12px]">Items too large for the Recycle Bin may be deleted permanently; Windows will warn you first. PULSE never deletes anything automatically.</div>
      </div>,
      "Move to Recycle Bin",
      true,
    );
    if (!ok) return;
    try {
      await api.recycle(paths);
      toast({ kind: "good", title: "Moved to Recycle Bin", body: "Rescan the folder to update sizes." });
      onChanged?.();
    } catch (e) {
      toast({ kind: "bad", title: "Could not move to Recycle Bin", body: String(e) });
    }
  };

  const itemsFor = (path: string, isDir: boolean, extra: MenuItem[] = [], size?: number): MenuItem[] => [
    ...extra,
    ...(extra.length ? [{ label: "", sep: true } as MenuItem] : []),
    { label: isDir ? "Open folder in Explorer" : "Show in Explorer", onClick: () => run(isDir ? api.openFolder(path) : api.reveal(path)) },
    ...(isDir ? [{ label: "Reveal in parent folder", onClick: () => run(api.reveal(path)) }] : [{ label: "Open containing folder", onClick: () => run(api.reveal(path)) }]),
    { label: "Copy full path", onClick: () => copy(path) },
    { label: "Properties…", onClick: () => run(api.props(path)) },
    { label: "", sep: true },
    { label: "Move to Recycle Bin…", danger: true, onClick: () => recycle([path], size) },
  ];
  return { node, copy, run, recycle, itemsFor };
}
