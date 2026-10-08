<script lang="ts" module>
  import type uPlot from "uplot";

  /** One line on a time-series chart. */
  export interface SeriesSpec {
    label: string;
    /** CSS variable for the colour, e.g. "--power". */
    color: string;
    unit: string;
    /** Scale key; series with different keys get their own axis. */
    scale?: string;
    digits?: number;
    fill?: boolean;
    width?: number;
  }

  // Charts in the same group share the cursor (uPlot's sync) and the x
  // zoom (propagated in the setScale hook below).
  const groups = new Map<string, Set<uPlot>>();
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import UPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { cssVar, theme } from "$lib/theme.svelte";

  interface Props {
    /** Accessible name, e.g. "Power and W' balance". */
    title: string;
    /** x values then one array per series (NaN or null = gap). */
    data: uPlot.AlignedData;
    series: SeriesSpec[];
    height?: number;
    group?: string;
    /** Formats x values for the axis and readout. */
    xFormat?: (x: number) => string;
    /** x is seconds into the ride (true) or Unix seconds dates (false). */
    xIsDuration?: boolean;
  }

  let { title, data, series, height = 200, group, xFormat = (x) => String(x), xIsDuration = true }: Props = $props();

  let el: HTMLDivElement;
  let plot: uPlot | null = null;
  let readout = $state("");
  let cursorIdx = 0;

  function options(width: number): uPlot.Options {
    const text = cssVar("--text-muted");
    const grid = cssVar("--border");
    const scales = [...new Set(series.map((s) => s.scale ?? "y"))];
    return {
      width,
      height,
      cursor: {
        sync: group ? { key: group, setSeries: false } : undefined,
        drag: { x: true, y: false, setScale: true },
        points: { size: 6 },
      },
      legend: { show: true, live: true },
      scales: { x: { time: !xIsDuration } },
      axes: [
        {
          stroke: text,
          grid: { stroke: grid, width: 1 },
          ticks: { stroke: grid },
          values: xIsDuration ? (_u: uPlot, vals: number[]) => vals.map((v) => xFormat(v)) : undefined,
        },
        ...scales.map((scale, i) => ({
          scale,
          side: i === 0 ? 3 : 1,
          stroke: text,
          grid: { show: i === 0, stroke: grid, width: 1 },
          ticks: { stroke: grid },
          size: 56,
          label: series.find((s) => (s.scale ?? "y") === scale)?.unit,
        })),
      ],
      series: [
        { label: xIsDuration ? "Time" : "Date", value: (_u: uPlot, v: number | null) => (v == null ? "—" : xFormat(v)) },
        ...series.map((s) => ({
          label: s.label,
          scale: s.scale ?? "y",
          stroke: cssVar(s.color),
          width: s.width ?? 1.5,
          fill: s.fill ? `color-mix(in srgb, ${cssVar(s.color)} 18%, transparent)` : undefined,
          value: (_u: uPlot, v: number | null) =>
            v == null || Number.isNaN(v) ? "—" : `${v.toFixed(s.digits ?? 0)} ${s.unit}`,
          spanGaps: false,
        })),
      ],
      hooks: {
        setScale: [
          (u: uPlot, key: string) => {
            if (key !== "x" || !group) return;
            const { min, max } = u.scales.x ?? {};
            if (min == null || max == null) return;
            for (const other of groups.get(group) ?? []) {
              if (other !== u && (other.scales.x?.min !== min || other.scales.x?.max !== max)) {
                other.setScale("x", { min, max });
              }
            }
          },
        ],
        setCursor: [
          (u: uPlot) => {
            if (u.cursor.idx != null) cursorIdx = u.cursor.idx;
          },
        ],
      },
    };
  }

  function build() {
    if (plot) {
      if (group) groups.get(group)?.delete(plot);
      plot.destroy();
    }
    plot = new UPlot(options(Math.max(el.clientWidth, 200)), data, el);
    if (group) {
      if (!groups.has(group)) groups.set(group, new Set());
      groups.get(group)?.add(plot);
    }
  }

  function describe(idx: number) {
    const x = data[0][idx];
    if (x == null) return;
    const parts = series.map((s, i) => {
      const v = data[i + 1]?.[idx];
      return `${s.label} ${v == null || Number.isNaN(v) ? "no data" : `${v.toFixed(s.digits ?? 0)} ${s.unit}`}`;
    });
    readout = `${xFormat(x)}: ${parts.join(", ")}`;
  }

  /** Keyboard: arrows move the cursor (shift = 10x), +/- zoom, Esc resets. */
  function onKey(e: KeyboardEvent) {
    if (!plot) return;
    const xs = data[0];
    const n = xs.length;
    if (n === 0) return;
    const first = xs[0] ?? 0;
    const last = xs[n - 1] ?? 0;
    const step = Math.max(1, Math.round(n / 200)) * (e.shiftKey ? 10 : 1);
    const min = plot.scales.x?.min ?? first;
    const max = plot.scales.x?.max ?? last;
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      cursorIdx = Math.min(n - 1, Math.max(0, cursorIdx + (e.key === "ArrowRight" ? step : -step)));
      const x = xs[cursorIdx] ?? first;
      plot.setCursor({ left: plot.valToPos(x, "x"), top: plot.over.clientHeight / 2 });
      describe(cursorIdx);
    } else if (e.key === "+" || e.key === "=" || e.key === "-") {
      const center = xs[cursorIdx] ?? (min + max) / 2;
      const half = ((max - min) / 2) * (e.key === "-" ? 2 : 0.5);
      plot.setScale("x", { min: Math.max(first, center - half), max: Math.min(last, center + half) });
    } else if (e.key === "Escape" || e.key === "0") {
      plot.setScale("x", { min: first, max: last });
    } else {
      return;
    }
    e.preventDefault();
  }

  onMount(() => {
    build();
    const ro = new ResizeObserver(() => plot?.setSize({ width: Math.max(el.clientWidth, 200), height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (group && plot) groups.get(group)?.delete(plot);
      plot?.destroy();
      plot = null;
    };
  });

  // Rebuild when the data, series or theme change.
  $effect(() => {
    void theme.version;
    void data;
    void series;
    if (el && plot) build();
  });
</script>

<figure class="ts">
  <figcaption class="visually-hidden">{title}</figcaption>
  <!-- role="application" is the ARIA role for a custom keyboard widget: the
       chart handles arrow/zoom keys itself (see onKey). Svelte's checker
       doesn't count it as interactive, hence the ignore. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="plot"
    bind:this={el}
    tabindex="0"
    role="application"
    aria-label="{title} chart. Arrow keys move the cursor, plus and minus zoom, Escape resets."
    onkeydown={onKey}
  ></div>
  <p class="visually-hidden" aria-live="polite">{readout}</p>
</figure>

<style>
  .ts {
    margin: 0;
    min-width: 0;
  }
  .plot {
    width: 100%;
    min-width: 0;
  }
  .plot :global(.u-legend) {
    font-size: 0.85rem;
    color: var(--text);
    text-align: left;
  }
  .plot :global(.u-select) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
</style>
