import { ReactNode, useMemo } from "react";
import type { Row } from "../lib/types";
import { bytes, CAT_COLORS, CAT_NAMES, fmtDate, num } from "../lib/format";
import { cx, Icon } from "./ui";

export type SortKey = "name" | "size" | "logical" | "files" | "mtime" | "type";

export function FileTable({
  rows,
  parentTotal,
  showPath = false,
  sort,
  desc,
  onSort,
  onOpen,
  onMenu,
  empty,
  selected,
  onSelect,
  metric = "alloc",
  maxHeight,
}: {
  rows: Row[];
  parentTotal?: number;
  showPath?: boolean;
  sort?: SortKey;
  desc?: boolean;
  onSort?: (k: SortKey) => void;
  onOpen?: (r: Row) => void;
  onMenu?: (e: React.MouseEvent, r: Row) => void;
  empty?: ReactNode;
  selected?: Set<number>;
  onSelect?: (id: number, on: boolean) => void;
  metric?: "alloc" | "logical";
  maxHeight?: number | string;
}) {
  const max = useMemo(() => rows.reduce((m, r) => Math.max(m, metric === "alloc" ? r.alloc : r.size), 1), [rows, metric]);
  const Th = ({ k, children, right, w }: { k?: SortKey; children: ReactNode; right?: boolean; w?: string }) => (
    <th style={{ width: w }} className={cx("sticky top-0 z-[1] bg-surface px-3 py-2 text-[11px] font-medium text-muted uppercase tracking-wide border-b border-line whitespace-nowrap", right ? "text-right" : "text-left", k && onSort && "cursor-pointer hover:text-ink")} onClick={() => k && onSort?.(k)}>
      {children}
      {k && sort === k && <span className="ml-1 text-accent">{desc ? "↓" : "↑"}</span>}
    </th>
  );
  if (!rows.length) return <div className="py-10 text-center text-muted text-[13px]">{empty ?? "Nothing to show."}</div>;
  return (
    <div className="overflow-auto" style={{ maxHeight: maxHeight ?? "none" }}>
      <table className="w-full border-collapse text-[12.5px]">
        <thead>
          <tr>
            {onSelect && <Th w="28px"> </Th>}
            <Th k="name">Name</Th>
            <Th k="size" right w="190px">
              {metric === "alloc" ? "Allocated" : "Logical"}
            </Th>
            <Th k="logical" right w="90px">
              {metric === "alloc" ? "Logical" : "Allocated"}
            </Th>
            {parentTotal ? (
              <Th right w="64px">
                %
              </Th>
            ) : null}
            <Th k="files" right w="80px">
              Files
            </Th>
            <Th k="mtime" w="100px">
              Modified
            </Th>
            <Th k="type" w="130px">
              Type
            </Th>
            <Th w="34px"> </Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const v = metric === "alloc" ? r.alloc : r.size;
            const other = metric === "alloc" ? r.size : r.alloc;
            return (
              <tr key={r.id} className={cx("group border-b border-line/60 hover:bg-surface2", selected?.has(r.id) && "bg-accentsoft")} onContextMenu={(e) => onMenu?.(e, r)} onDoubleClick={() => onOpen?.(r)}>
                {onSelect && (
                  <td className="px-3 py-1.5">
                    <input type="checkbox" checked={selected?.has(r.id) ?? false} onChange={(e) => onSelect(r.id, e.target.checked)} />
                  </td>
                )}
                <td className="px-3 py-1.5 max-w-0">
                  <div className="flex items-center gap-2 min-w-0">
                    {r.isDir ? <Icon.folder className="shrink-0 text-accent/80" /> : <span className="shrink-0 w-2.5 h-2.5 rounded-sm" style={{ background: CAT_COLORS[r.cat] }} />}
                    <div className="min-w-0">
                      <button className={cx("block truncate max-w-full text-left", r.isDir ? "text-ink hover:text-accent" : "text-ink cursor-default")} onClick={() => r.isDir && onOpen?.(r)} title={r.path || r.name}>
                        {r.name}
                      </button>
                      {showPath && r.path && (
                        <div className="truncate text-[11px] text-faint" title={r.path}>
                          {r.path}
                        </div>
                      )}
                    </div>
                  </div>
                </td>
                <td className="px-3 py-1.5 text-right num">
                  <div className="flex items-center justify-end gap-2">
                    <div className="w-16 h-1.5 rounded-full bg-surface3 overflow-hidden">
                      <div className="h-full rounded-full bg-accent/70" style={{ width: `${Math.max(1, (v / max) * 100)}%` }} />
                    </div>
                    <span className="w-16 text-right">{bytes(v)}</span>
                  </div>
                </td>
                <td className="px-3 py-1.5 text-right num text-muted">{bytes(other)}</td>
                {parentTotal ? <td className="px-3 py-1.5 text-right num text-muted">{parentTotal > 0 ? ((v * 100) / parentTotal).toFixed(1) : "0"}%</td> : null}
                <td className="px-3 py-1.5 text-right num text-muted">{r.isDir ? num(r.files) : ""}</td>
                <td className="px-3 py-1.5 text-muted whitespace-nowrap">{fmtDate(r.mtime)}</td>
                <td className="px-3 py-1.5 text-muted truncate">{r.isDir ? "Folder" : r.ext ? `${r.ext.toUpperCase()} · ${CAT_NAMES[r.cat]}` : CAT_NAMES[r.cat]}</td>
                <td className="px-1 py-1.5 text-right">
                  <button className="opacity-0 group-hover:opacity-100 text-muted hover:text-ink p-1 rounded hover:bg-line" onClick={(e) => onMenu?.(e, r)} title="Actions">
                    <Icon.more />
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
