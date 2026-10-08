<script lang="ts">
  import { onMount } from "svelte";
  import type { ECharts, EChartsCoreOption } from "echarts/core";
  import { cssVar, theme } from "$lib/theme.svelte";

  interface Props {
    /** Accessible name. */
    title: string;
    /** Builds the option from theme colours (called again when the theme changes). */
    option: (color: (cssVariable: string) => string) => EChartsCoreOption;
    height?: number;
  }

  let { title, option, height = 260 }: Props = $props();
  let el: HTMLDivElement;
  let chart = $state.raw<ECharts | null>(null);

  function render() {
    if (!chart) return;
    chart.setOption(
      {
        backgroundColor: "transparent",
        textStyle: { color: cssVar("--text"), fontFamily: cssVar("--font") },
        // A generated text description of the chart for screen readers.
        aria: { enabled: true },
        animation: false,
        ...option(cssVar),
      },
      true,
    );
  }

  onMount(() => {
    let disposed = false;
    let ro: ResizeObserver | undefined;
    // Loaded on demand: the box below already has its final size, so the
    // page doesn't shift when the chart appears.
    void import("$lib/charts/echarts").then(({ echarts }) => {
      if (disposed) return;
      chart = echarts.init(el, undefined, { renderer: "canvas" });
      ro = new ResizeObserver(() => chart?.resize());
      ro.observe(el);
    });
    return () => {
      disposed = true;
      ro?.disconnect();
      chart?.dispose();
      chart = null;
    };
  });

  $effect(() => {
    void theme.version;
    void option;
    void chart;
    render();
  });
</script>

<figure class="ec">
  <figcaption class="visually-hidden">{title}</figcaption>
  <div bind:this={el} style:height="{height}px" role="img" aria-label={title}></div>
</figure>

<style>
  .ec {
    margin: 0;
    min-width: 0;
  }
  div {
    width: 100%;
  }
</style>
