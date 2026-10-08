<script lang="ts">
  import { onMount } from "svelte";
  import * as echarts from "echarts/core";
  import { BarChart, LineChart } from "echarts/charts";
  import { AriaComponent, GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from "echarts/components";
  import { CanvasRenderer } from "echarts/renderers";
  import { cssVar, theme } from "$lib/theme.svelte";

  // Only what the app uses: keeps ECharts' share of the bundle small.
  echarts.use([BarChart, LineChart, GridComponent, TooltipComponent, LegendComponent, MarkLineComponent, AriaComponent, CanvasRenderer]);

  interface Props {
    /** Accessible name. */
    title: string;
    /** Builds the option from theme colours (called again when the theme changes). */
    option: (color: (cssVariable: string) => string) => echarts.EChartsCoreOption;
    height?: number;
  }

  let { title, option, height = 260 }: Props = $props();
  let el: HTMLDivElement;
  let chart: echarts.ECharts | null = null;

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
    chart = echarts.init(el, undefined, { renderer: "canvas" });
    render();
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => {
      ro.disconnect();
      chart?.dispose();
      chart = null;
    };
  });

  $effect(() => {
    void theme.version;
    void option;
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
