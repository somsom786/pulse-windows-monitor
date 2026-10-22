import { useEffect, useMemo, useRef, useState } from "react";
import type { Tm } from "../lib/types";
import { bytes, CAT_COLORS, CAT_NAMES, FOLDER_COLOR, num } from "../lib/format";

interface Rect {
  node: Tm;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  path: string;
  hasKids: boolean;
}

function worst(row: number[], side: number) {
  const s = row.reduce((a, b) => a + b, 0);
  const mx = Math.max(...row);
  const mn = Math.min(...row);
  return Math.max((side * side * mx) / (s * s), (s * s) / (side * side * mn));
}

function squarify(vals: number[], x: number, y: number, w: number, h: number) {
  const out: { x: number; y: number; w: number; h: number }[] = [];
  const total = vals.reduce((a, b) => a + b, 0);
  if (total <= 0 || w <= 0 || h <= 0) return out;
  const scale = (w * h) / total;
  const areas = vals.map((v) => v * scale);
  let i = 0;
  while (i < areas.length) {
    const side = Math.min(w, h);
    let row = [areas[i]];
    let wr = worst(row, side);
    let j = i + 1;
    while (j < areas.length) {
      const nr = [...row, areas[j]];
      const nw = worst(nr, side);
      if (nw > wr) break;
      row = nr;
      wr = nw;
      j++;
    }
    const rs = row.reduce((a, b) => a + b, 0);
    if (w >= h) {
      const cw = rs / h;
      let yy = y;
      for (const a of row) {
        const hh = a / cw;
        out.push({ x, y: yy, w: cw, h: hh });
        yy += hh;
      }
      x += cw;
      w -= cw;
    } else {
      const rh = rs / w;
      let xx = x;
      for (const a of row) {
        const ww = a / rh;
        out.push({ x: xx, y, w: ww, h: rh });
        xx += ww;
      }
      y += rh;
      h -= rh;
    }
    i = j;
  }
  return out;
}

function layout(node: Tm, x: number, y: number, w: number, h: number, depth: number, path: string, out: Rect[]) {
  const hasKids = !!node.children && node.children.length > 0;
  out.push({ node, x, y, w, h, depth, path, hasKids });
  if (!hasKids || w < 22 || h < 22) return;
  const pad = depth === 0 ? 0 : 2;
  const header = depth >= 1 && h > 44 && w > 64 ? 16 : 0;
  const ix = x + pad,
    iy = y + pad + header,
    iw = w - pad * 2,
    ih = h - pad * 2 - header;
  if (iw < 8 || ih < 8) return;
  const kids = node.children!.filter((k) => k.value > 0);
  const rects = squarify(
    kids.map((k) => k.value),
    ix,
    iy,
    iw,
    ih,
  );
  rects.forEach((r, i) => {
    if (r.w < 1.2 || r.h < 1.2) return;
    const k = kids[i];
    const p = k.id < 0 ? path : path ? (path.endsWith("\\") ? path + k.name : path + "\\" + k.name) : k.name;
    layout(k, r.x, r.y, r.w, r.h, depth + 1, p, out);
  });
}

export interface TmPick {
  id: number;
  name: string;
  path: string;
  isDir: boolean;
}

export function Treemap({ data, rootPath, metric, catFilter, onDrill, onContext, onSelect, height = 420 }: { data: Tm; rootPath: string; metric: "alloc" | "logical"; catFilter: number | null; onDrill: (id: number) => void; onContext?: (e: React.MouseEvent, p: TmPick) => void; onSelect?: (p: TmPick | null) => void; height?: number }) {
  const wrap = useRef<HTMLDivElement>(null);
  const cv = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: height });
  const [hover, setHover] = useState<{ r: Rect; x: number; y: number } | null>(null);

  useEffect(() => {
    if (!wrap.current) return;
    const ro = new ResizeObserver((es) => {
      const r = es[0].contentRect;
      setSize({ w: Math.max(200, Math.floor(r.width)), h: height });
    });
    ro.observe(wrap.current);
    return () => ro.disconnect();
  }, [height]);

  const rects = useMemo(() => {
    const out: Rect[] = [];
    layout(data, 0, 0, size.w, size.h, 0, rootPath.replace(/\\$/, "") + (rootPath.endsWith("\\") && rootPath.length <= 3 ? "\\" : ""), out);
    return out;
  }, [data, size, rootPath]);

  useEffect(() => {
    const c = cv.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    const ctx = c.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const styles = getComputedStyle(document.documentElement);
    const surface = styles.getPropertyValue("--surface-2").trim() || "#141821";
    const border = styles.getPropertyValue("--bg").trim() || "#090b0f";
    const textc = "#0b0e14";
    ctx.clearRect(0, 0, size.w, size.h);
    ctx.fillStyle = surface;
    ctx.fillRect(0, 0, size.w, size.h);
    ctx.font = "500 11px 'Inter Variable', 'Segoe UI', sans-serif";
    ctx.textBaseline = "top";

    for (const r of rects) {
      const n = r.node;
      if (r.depth === 0) continue;
      const x = r.x + 0.5,
        y = r.y + 0.5,
        w = r.w - 1,
        h = r.h - 1;
      if (w < 1 || h < 1) continue;
      const isContainer = r.hasKids;
      if (isContainer) {
        ctx.fillStyle = "rgba(120,140,170,0.10)";
        ctx.fillRect(x, y, w, h);
        ctx.strokeStyle = "rgba(140,160,190,0.35)";
        ctx.lineWidth = 1;
        ctx.strokeRect(x, y, w, h);
        if (h > 44 && w > 64) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(x, y, w, 16);
          ctx.clip();
          ctx.fillStyle = styles.getPropertyValue("--muted").trim() || "#8d96a9";
          ctx.fillText(`${n.name}  ·  ${bytes(n.value)}`, x + 5, y + 3);
          ctx.restore();
        }
        continue;
      }
      let color = n.id < 0 ? "#4a5163" : n.isDir ? FOLDER_COLOR : CAT_COLORS[n.cat] ?? CAT_COLORS[0];
      const dim = catFilter != null && !n.isDir && n.id >= 0 && n.cat !== catFilter;
      ctx.globalAlpha = dim ? 0.16 : 1;
      ctx.fillStyle = color;
      ctx.fillRect(x, y, w, h);
      // subtle top highlight for depth
      const g = ctx.createLinearGradient(0, y, 0, y + h);
      g.addColorStop(0, "rgba(255,255,255,0.16)");
      g.addColorStop(1, "rgba(0,0,0,0.12)");
      ctx.fillStyle = g;
      ctx.fillRect(x, y, w, h);
      ctx.strokeStyle = border;
      ctx.globalAlpha = dim ? 0.3 : 0.55;
      ctx.lineWidth = 1;
      ctx.strokeRect(x, y, w, h);
      ctx.globalAlpha = dim ? 0.2 : 1;
      if (n.id < 0) {
        ctx.strokeStyle = "rgba(255,255,255,0.12)";
        ctx.beginPath();
        for (let k = -h; k < w; k += 7) {
          ctx.moveTo(x + Math.max(0, k), y + Math.max(0, -k));
          ctx.lineTo(x + Math.min(w, k + h), y + Math.min(h, h - Math.max(0, w - k)));
        }
        ctx.stroke();
      }
      if (w > 46 && h > 16 && !dim) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(x + 1, y + 1, w - 2, h - 2);
        ctx.clip();
        ctx.fillStyle = textc;
        ctx.globalAlpha = 0.88;
        ctx.fillText(n.name, x + 4, y + 3);
        if (h > 30) {
          ctx.globalAlpha = 0.65;
          ctx.fillText(bytes(n.value), x + 4, y + 16);
        }
        ctx.restore();
      }
      ctx.globalAlpha = 1;
    }
    if (hover) {
      const r = hover.r;
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 2;
      ctx.strokeRect(r.x + 1, r.y + 1, Math.max(1, r.w - 2), Math.max(1, r.h - 2));
    }
  }, [rects, size, hover, catFilter]);

  const pick = (e: React.MouseEvent) => {
    const b = cv.current!.getBoundingClientRect();
    const mx = e.clientX - b.left,
      my = e.clientY - b.top;
    let best: Rect | null = null;
    for (let i = rects.length - 1; i >= 0; i--) {
      const r = rects[i];
      if (r.depth === 0) continue;
      if (mx >= r.x && mx < r.x + r.w && my >= r.y && my < r.y + r.h) {
        // prefer the deepest rect: later entries in DFS order are deeper or siblings; first hit from the end is correct
        best = r;
        break;
      }
    }
    return { best, mx, my };
  };

  const total = data.value || 1;
  return (
    <div ref={wrap} className="relative rounded-lg overflow-hidden border border-line bg-surface2" style={{ height }}>
      <canvas
        ref={cv}
        style={{ width: size.w, height: size.h, display: "block", cursor: hover ? "pointer" : "default" }}
        onMouseMove={(e) => {
          const { best, mx, my } = pick(e);
          setHover(best ? { r: best, x: mx, y: my } : null);
        }}
        onMouseLeave={() => setHover(null)}
        onClick={(e) => {
          const { best } = pick(e);
          if (!best || best.node.id < 0) return;
          const p: TmPick = { id: best.node.id, name: best.node.name, path: best.path, isDir: best.node.isDir };
          onSelect?.(p);
          if (best.node.isDir) onDrill(best.node.id);
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          const { best } = pick(e);
          if (!best || best.node.id < 0) return;
          onContext?.(e, { id: best.node.id, name: best.node.name, path: best.path, isDir: best.node.isDir });
        }}
      />
      {hover && (
        <div
          className="pointer-events-none absolute z-10 max-w-[360px] rounded-lg border border-line2 bg-surface3/95 px-3 py-2 text-[12px] shadow-xl selectable"
          style={{ left: Math.min(hover.x + 14, size.w - 300), top: Math.min(hover.y + 14, size.h - 120) }}
        >
          <div className="font-semibold text-ink break-all">{hover.r.node.name}</div>
          {hover.r.node.id >= 0 && <div className="text-muted break-all text-[11px] mb-1">{hover.r.path}</div>}
          <div className="num text-ink">
            {bytes(hover.r.node.value)} <span className="text-muted">· {((hover.r.node.value * 100) / total).toFixed(hover.r.node.value / total < 0.01 ? 2 : 1)}% of view</span>
          </div>
          {hover.r.node.id >= 0 && (
            <div className="num text-[11px] text-muted">
              {metric === "alloc" ? `Logical ${bytes(hover.r.node.size)}` : `Allocated ${bytes(hover.r.node.alloc)}`}
              {hover.r.node.isDir ? ` · ${num(hover.r.node.files)} files` : ` · ${CAT_NAMES[hover.r.node.cat] ?? ""}`}
            </div>
          )}
          {hover.r.node.isDir && hover.r.node.id >= 0 && <div className="text-[11px] text-accent mt-0.5">Click to zoom in · right-click for actions</div>}
        </div>
      )}
    </div>
  );
}
