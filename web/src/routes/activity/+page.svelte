<script lang="ts">
  import { browser } from "$app/environment";
  import { base } from "$app/paths";
  import { page } from "$app/state";
  import type uPlot from "uplot";
  import { app } from "$lib/state.svelte";
  import { getActivity, type StoredActivity } from "$lib/db";
  import { call } from "$lib/worker/client";
  import { dateTime, duration, durationLabel, num } from "$lib/format";
  import type { AnalyzedActivity, Streams } from "$lib/types";
  import TimeSeries, { type SeriesSpec } from "$lib/components/TimeSeries.svelte";
  import EChart from "$lib/components/EChart.svelte";
  import DataTable from "$lib/components/DataTable.svelte";

  // Query parameters exist only in the browser (the page is prerendered).
  const id = $derived(browser ? (page.url.searchParams.get("id") ?? "") : "");

  let stored = $state<StoredActivity | null>(null);
  let result = $state<AnalyzedActivity | null>(null);
  let streams = $state.raw<Streams | null>(null);
  let error = $state("");

  $effect(() => {
    if (!app.ready || !id) return;
    const settings = $state.snapshot(app.settings);
    error = "";
    void (async () => {
      const a = await getActivity(id);
      if (!a) {
        error = "This activity isn't stored in this browser.";
        return;
      }
      stored = a;
      const copy = a.bytes.slice(0);
      try {
        const r = await call("analyze", [copy, settings, true], [copy]);
        result = r.activity;
        streams = r.streams;
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    })();
  });

  const s = $derived(result?.summary);

  /** x = elapsed seconds; a null point at each pause so lines break there. */
  function series(values: ArrayLike<number>, missing: (v: number) => boolean, scale = 1): [number[], (number | null)[]] {
    const st = streams;
    if (!st) return [[], []];
    const xs: number[] = [];
    const ys: (number | null)[] = [];
    for (let i = 0; i < st.elapsed.length; i++) {
      const e = st.elapsed[i]!;
      if (i > 0 && e - st.elapsed[i - 1]! > 1) {
        xs.push(e - 0.5);
        ys.push(null);
      }
      xs.push(e);
      const v = values[i]!;
      ys.push(missing(v) ? null : v * scale);
    }
    return [xs, ys];
  }

  const powerData = $derived.by((): uPlot.AlignedData => {
    if (!streams) return [[]];
    const [xs, p] = series(streams.power, () => false);
    const w =
      streams.wPrimeBalance.length === streams.power.length
        ? series(streams.wPrimeBalance, Number.isNaN, 0.001)[1]
        : xs.map(() => null);
    return [xs, p, w];
  });
  const hrData: uPlot.AlignedData = $derived(streams ? series(streams.heartRate, (v) => v === 0) : [[]]);
  const cadData: uPlot.AlignedData = $derived(streams ? series(streams.cadence, (v) => v === 0) : [[]]);
  const altData: uPlot.AlignedData = $derived(streams ? series(streams.altitude, Number.isNaN) : [[]]);
  const has = (d: uPlot.AlignedData) => (d[1] ?? []).some((v) => v != null);

  const powerSeries: SeriesSpec[] = [
    { label: "Power", color: "--power", unit: "W" },
    { label: "W′ balance", color: "--wbal", unit: "kJ", scale: "wbal", digits: 1, width: 2 },
  ];
  const hrSeries: SeriesSpec[] = [{ label: "Heart rate", color: "--hr", unit: "bpm" }];
  const cadSeries: SeriesSpec[] = [{ label: "Cadence", color: "--cadence", unit: "rpm" }];
  const altSeries: SeriesSpec[] = [{ label: "Altitude", color: "--altitude", unit: "m", fill: true }];

  const powerZoneNames = ["Z1 Recovery", "Z2 Endurance", "Z3 Tempo", "Z4 Threshold", "Z5 VO2max", "Z6 Anaerobic", "Z7 Neuromuscular"];
  const hrZoneNames = ["Z1", "Z2", "Z3", "Z4", "Z5a", "Z5b", "Z5c"];

  function zoneOption(seconds: number[], names: string[], colorVar: string) {
    return (c: (v: string) => string) => ({
      grid: { left: 120, right: 50, top: 10, bottom: 30 },
      tooltip: { trigger: "axis", valueFormatter: (v: number) => `${num(v, 1)} min` },
      xAxis: { type: "value", name: "minutes", nameLocation: "middle", nameGap: 22, axisLabel: { color: c("--text-muted") }, splitLine: { lineStyle: { color: c("--border") } } },
      yAxis: { type: "category", inverse: true, data: names.slice(0, seconds.length), axisLabel: { color: c("--text") } },
      series: [{ type: "bar", data: seconds.map((x) => +(x / 60).toFixed(1)), itemStyle: { color: c(colorVar) }, label: { show: true, position: "right", color: c("--text-muted"), formatter: ({ value }: { value: number }) => `${value}` } }],
    });
  }

  const curvePoints = $derived(
    result ? Array.from(result.curveDurations, (d, i) => [d, result!.curveWatts[i] ?? Number.NaN]).filter(([, w]) => Number.isFinite(w)) : [],
  );
  const curveOption = $derived((c: (v: string) => string) => ({
    grid: { left: 55, right: 20, top: 20, bottom: 45 },
    tooltip: { trigger: "axis", formatter: (p: { value: [number, number] }[]) => `${durationLabel(p[0]!.value[0])}: ${num(p[0]!.value[1])} W` },
    xAxis: { type: "log", logBase: 10, name: "duration", nameLocation: "middle", nameGap: 28, axisLabel: { color: c("--text-muted"), formatter: (v: number) => durationLabel(v) }, splitLine: { lineStyle: { color: c("--border") } } },
    yAxis: { type: "value", name: "W", axisLabel: { color: c("--text-muted") }, splitLine: { lineStyle: { color: c("--border") } } },
    series: [{ type: "line", showSymbol: false, data: curvePoints, lineStyle: { color: c("--power"), width: 2 }, areaStyle: { color: c("--power"), opacity: 0.12 } }],
  }));
</script>

<svelte:head><title>{stored?.name ?? "Activity"} · zerofit</title></svelte:head>

<p><a href="{base}/activities/">← All activities</a></p>

{#if error}
  <p class="error" role="alert">{error}</p>
{:else if !stored || !result || !s}
  <!-- Placeholders with the final layout's heights, so nothing jumps. -->
  <h1 class="placeholder-title">Activity</h1>
  <p class="muted">Analyzing in your browser…</p>
  <div class="card skeleton" style:height="300px"></div>
  <div class="card skeleton" style:height="720px"></div>
{:else}
  <h1>{stored.name}</h1>
  <p class="muted">
    {dateTime(stored.startMs)} · {result.sport ?? "unknown sport"} ·
    <span data-testid="analysis-ms">analyzed in {result.analysisMs.toFixed(0)} ms on this device</span>
  </p>

  <section class="card" aria-labelledby="sum-h">
    <h2 id="sum-h">Summary</h2>
    <div class="stats">
      <div class="stat"><div class="label">Moving time</div><div class="value">{duration(s.moving_time_s)}</div></div>
      <div class="stat"><div class="label">Elapsed</div><div class="value">{duration(s.elapsed_time_s)}</div></div>
      {#if s.has_power}
        <div class="stat"><div class="label">Avg power</div><div class="value">{num(s.average_power)}<span class="unit">W</span></div></div>
        <div class="stat"><div class="label">Normalized</div><div class="value" data-testid="np">{num(s.normalized_power)}<span class="unit">W</span></div></div>
        <div class="stat"><div class="label">Max power</div><div class="value">{num(s.max_power)}<span class="unit">W</span></div></div>
        <div class="stat"><div class="label">IF</div><div class="value">{num(s.intensity_factor, 2)}</div></div>
        <div class="stat"><div class="label">TSS</div><div class="value">{num(s.tss)}</div></div>
        <div class="stat"><div class="label">VI</div><div class="value">{num(s.variability_index, 2)}</div></div>
        <div class="stat"><div class="label">Work</div><div class="value">{num(s.work_kj)}<span class="unit">kJ</span></div></div>
        <div class="stat"><div class="label">Avg W/kg</div><div class="value">{num(s.average_watts_per_kg, 2)}</div></div>
        <div class="stat"><div class="label">Min W′bal</div><div class="value">{num((s.min_w_prime_balance ?? Number.NaN) / 1000, 1)}<span class="unit">kJ</span></div></div>
      {/if}
      <div class="stat"><div class="label">Avg HR</div><div class="value">{num(s.average_hr)}<span class="unit">bpm</span></div></div>
      <div class="stat"><div class="label">Max HR</div><div class="value">{num(s.max_hr)}<span class="unit">bpm</span></div></div>
      <div class="stat"><div class="label">hrTSS</div><div class="value">{num(s.hr_tss)}</div></div>
      {#if s.has_power}
        <div class="stat"><div class="label">EF</div><div class="value">{num(s.efficiency_factor, 2)}</div></div>
        <div class="stat"><div class="label">Pa:HR drift</div><div class="value">{num(s.decoupling_pct, 1)}<span class="unit">%</span></div></div>
      {/if}
    </div>
    <p class="muted small">
      FTP {app.settings.ftp} W, LTHR {app.settings.lthr} bpm (change in <a href="{base}/settings/">Settings</a>). TSS uses moving time, as
      intervals.icu does.
    </p>
  </section>

  {#if streams}
    <section class="card charts" aria-labelledby="charts-h">
      <h2 id="charts-h">Ride</h2>
      <p class="muted small">Drag across a chart to zoom all of them; double-click to reset. With the keyboard: focus a chart, then arrows, + and −, Escape.</p>
      {#if s.has_power}
        <TimeSeries title="Power and W′ balance" data={powerData} series={powerSeries} group="activity" xFormat={duration} height={220} />
      {/if}
      {#if has(hrData)}
        <TimeSeries title="Heart rate" data={hrData} series={hrSeries} group="activity" xFormat={duration} height={150} />
      {/if}
      {#if has(cadData)}
        <TimeSeries title="Cadence" data={cadData} series={cadSeries} group="activity" xFormat={duration} height={130} />
      {/if}
      {#if has(altData)}
        <TimeSeries title="Altitude" data={altData} series={altSeries} group="activity" xFormat={duration} height={130} />
      {/if}
    </section>
  {/if}

  <div class="grid">
    {#if s.power_zone_seconds}
      <section class="card" aria-labelledby="pz-h">
        <h2 id="pz-h">Time in power zones</h2>
        <EChart title="Minutes in each power zone" option={zoneOption(s.power_zone_seconds, powerZoneNames, "--power")} height={250} />
        <DataTable caption="Time in power zones" headers={["Zone", "Time"]} rows={s.power_zone_seconds.map((x, i) => [powerZoneNames[i] ?? `Z${i + 1}`, duration(x)])} />
      </section>
    {/if}
    {#if s.hr_zone_seconds}
      <section class="card" aria-labelledby="hz-h">
        <h2 id="hz-h">Time in heart-rate zones</h2>
        <EChart title="Minutes in each heart-rate zone" option={zoneOption(s.hr_zone_seconds, hrZoneNames, "--hr")} height={250} />
        <DataTable caption="Time in heart-rate zones" headers={["Zone", "Time"]} rows={s.hr_zone_seconds.map((x, i) => [hrZoneNames[i] ?? `Z${i + 1}`, duration(x)])} />
      </section>
    {/if}
  </div>

  {#if curvePoints.length > 0}
    <section class="card" aria-labelledby="mmp-h">
      <h2 id="mmp-h">Power curve (mean-maximal power)</h2>
      <EChart title="Best average power for each duration in this ride" option={curveOption} height={280} />
      <DataTable
        caption="Power curve"
        headers={["Duration", "Watts"]}
        rows={s.power_curve.map((p) => [durationLabel(p.duration_s), num(p.watts)])}
      />
      {#if s.cp_fit}
        <p class="small">
          2-parameter fit of this ride: CP {num(s.cp_fit.cp)} ± {num(s.cp_fit.se_cp)} W, W′ {num(s.cp_fit.w_prime / 1000, 1)} kJ, R²
          {num(s.cp_fit.r_squared, 3)}. A single ride is rarely maximal at every duration; the <a href="{base}/fitness/">season curve</a> is more reliable.
        </p>
      {/if}
    </section>
  {/if}

  {#if s.resample_report}
    <details class="card">
      <summary>How the raw records were resampled</summary>
      <ul class="small">
        <li>{s.resample_report.records} records; {s.resample_report.duplicates} duplicate timestamps merged; {s.resample_report.out_of_order} out of order dropped</li>
        <li>{s.resample_report.gaps_filled} short gaps filled ({s.resample_report.gap_seconds_filled} s interpolated)</li>
        <li>{s.resample_report.pauses} pauses removed ({duration(s.resample_report.paused_seconds)})</li>
        <li>{s.resample_report.power_dropouts_repaired} s of power dropout repaired; {s.resample_report.power_dropout_seconds} s without power; {s.resample_report.power_spikes} spikes removed</li>
      </ul>
    </details>
  {/if}
{/if}

<style>
  h1 {
    overflow-wrap: anywhere;
  }
  .card,
  .grid {
    margin-bottom: 1rem;
  }
  .charts {
    display: grid;
    gap: 0.75rem;
  }
  .small {
    font-size: 0.85rem;
  }
  .error {
    color: var(--danger);
  }
  summary {
    cursor: pointer;
    min-height: 2rem;
  }
  .skeleton {
    background: var(--surface);
  }
</style>
