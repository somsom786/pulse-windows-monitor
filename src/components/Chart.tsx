import * as echarts from "echarts/core";
import { LineChart, BarChart, ScatterChart } from "echarts/charts";
import { GridComponent, TooltipComponent, MarkLineComponent, MarkAreaComponent, LegendComponent, DataZoomComponent, AxisPointerComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { useEffect, useRef } from "react";

echarts.use([LineChart, BarChart, ScatterChart, GridComponent, TooltipComponent, MarkLineComponent, MarkAreaComponent, LegendComponent, DataZoomComponent, AxisPointerComponent, CanvasRenderer]);

export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export interface Theme {
  text: string;
  muted: string;
  faint: string;
  line: string;
  surface3: string;
  accent: string;
  good: string;
  warn: string;
  bad: string;
}
export function theme(): Theme {
  return {
    text: cssVar("--text"),
    muted: cssVar("--muted"),
    faint: cssVar("--faint"),
    line: cssVar("--border"),
    surface3: cssVar("--surface-3"),
    accent: cssVar("--accent"),
    good: cssVar("--good"),
    warn: cssVar("--warn"),
    bad: cssVar("--bad"),
  };
}

export const SERIES = { cpu: "#38bdf8", ram: "#a78bfa", gpu: "#34d399", vram: "#f59e0b", temp: "#f87171", read: "#38bdf8", write: "#f472b6", rx: "#34d399", tx: "#60a5fa", commit: "#94a3b8" };

export function Chart({ option, height = 180, onClick, onZr, className, deps }: { option: any; height?: number | string; onClick?: (p: any) => void; onZr?: (e: any, chart: echarts.ECharts) => void; className?: string; deps?: any[] }) {
  const el = useRef<HTMLDivElement>(null);
  const chart = useRef<echarts.ECharts | null>(null);
  const clickRef = useRef(onClick);
  clickRef.current = onClick;
  const zrRef = useRef(onZr);
  zrRef.current = onZr;

  useEffect(() => {
    if (!el.current) return;
    const c = echarts.init(el.current, undefined, { renderer: "canvas" });
    chart.current = c;
    c.on("click", (p) => clickRef.current?.(p));
    c.getZr().on("click", (e: any) => zrRef.current?.(e, c));
    const ro = new ResizeObserver(() => c.resize());
    ro.observe(el.current);
    return () => {
      ro.disconnect();
      c.dispose();
      chart.current = null;
    };
  }, []);

  useEffect(() => {
    chart.current?.setOption(option, { notMerge: true, lazyUpdate: true });
  }, [option, ...(deps ?? [])]);

  return <div ref={el} className={className} style={{ height, width: "100%" }} />;
}

export function baseOption(t: Theme, opts: { yFormatter?: (v: number) => string; yMax?: number | "dataMax"; yMin?: number; legend?: boolean; zoom?: boolean; gridLeft?: number } = {}) {
  return {
    animation: false,
    backgroundColor: "transparent",
    textStyle: { color: t.muted, fontFamily: "Inter Variable, Segoe UI, sans-serif", fontSize: 11 },
    grid: { left: opts.gridLeft ?? 44, right: 12, top: opts.legend ? 28 : 10, bottom: opts.zoom ? 44 : 22 },
    xAxis: { type: "time", axisLine: { lineStyle: { color: t.line } }, axisTick: { show: false }, axisLabel: { color: t.faint, hideOverlap: true }, splitLine: { show: false } },
    yAxis: { type: "value", min: opts.yMin ?? 0, max: opts.yMax, axisLabel: { color: t.faint, formatter: opts.yFormatter }, splitLine: { lineStyle: { color: t.line, opacity: 0.6 } }, axisLine: { show: false } },
    legend: opts.legend ? { top: 0, right: 0, textStyle: { color: t.muted }, icon: "roundRect", itemWidth: 10, itemHeight: 3 } : undefined,
    tooltip: {
      trigger: "axis",
      backgroundColor: t.surface3,
      borderColor: t.line,
      textStyle: { color: t.text, fontSize: 12 },
      axisPointer: { type: "line", lineStyle: { color: t.faint } },
    },
  };
}

export function line(name: string, data: [number, number | null][], color: string, extra: any = {}) {
  return { name, type: "line", showSymbol: false, smooth: false, connectNulls: false, data, lineStyle: { width: 1.6, color }, itemStyle: { color }, areaStyle: extra.area ? { color, opacity: 0.12 } : undefined, ...extra };
}
