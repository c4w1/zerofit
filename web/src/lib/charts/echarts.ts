// ECharts with only the pieces the app uses. Imported lazily by
// EChart.svelte, so pages without such charts never download it and chart
// pages render their text before parsing it.
import * as echarts from "echarts/core";
import { BarChart, LineChart } from "echarts/charts";
import { AriaComponent, GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";

echarts.use([BarChart, LineChart, GridComponent, TooltipComponent, LegendComponent, MarkLineComponent, AriaComponent, CanvasRenderer]);

export { echarts };
