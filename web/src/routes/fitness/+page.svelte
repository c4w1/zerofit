<script lang="ts">
  import { base } from "$app/paths";
  import type uPlot from "uplot";
  import { app } from "$lib/state.svelte";
  import { computeFitness } from "$lib/fitness";
  import { dayIndexToDate, duration, durationLabel, isoDay, num } from "$lib/format";
  import type { CpFit, Fitness } from "$lib/types";
  import TimeSeries, { type SeriesSpec } from "$lib/components/TimeSeries.svelte";
  import EChart from "$lib/components/EChart.svelte";
  import DataTable from "$lib/components/DataTable.svelte";

  let fitness = $state.raw<Fitness | null>(null);
  let error = $state("");

  $effect(() => {
    if (!app.ready) return;
    const list = $state.snapshot(app.activities);
    if (list.length === 0) {
      fitness = null;
      return;
    }
    computeFitness(list, app.settings.w_prime)
      .then((f) => (fitness = f))
      .catch((e) => (error = String(e)));
  });

  const loadData = $derived.by((): uPlot.AlignedData => {
    if (!fitness) return [[]];
    const xs = fitness.days.map((_, i) => dayIndexToDate(fitness!.first_day + i).getTime() / 1000);
    return [xs, fitness.days.map((d) => d.ctl), fitness.days.map((d) => d.atl), fitness.days.map((d) => d.tsb)];
  });
  const loadSeries: SeriesSpec[] = [
    { label: "Fitness (CTL)", color: "--ctl", unit: "", width: 2 },
    { label: "Fatigue (ATL)", color: "--atl", unit: "" },
    { label: "Form (TSB)", color: "--tsb", unit: "", fill: true },
  ];

  const model = (fit: CpFit, t: number) => fit.w_prime / (t - (fit.k ?? 0)) + fit.cp;

  const curveOption = $derived((c: (v: string) => string) => {
    const pts = fitness?.season_curve ?? [];
    const ds = pts.map(([d]) => d).filter((d) => d >= 60 && d <= 3600);
    const lines: object[] = [
      { name: "Season best", type: "line", showSymbol: false, data: pts, lineStyle: { color: c("--power"), width: 2.5 }, areaStyle: { color: c("--power"), opacity: 0.1 } },
    ];
    if (fitness?.cp_2p) {
      const fit = fitness.cp_2p;
      lines.push({ name: "CP 2-parameter", type: "line", showSymbol: false, data: ds.map((d) => [d, model(fit, d)]), lineStyle: { color: c("--hr"), type: "dashed", width: 1.5 } });
    }
    if (fitness?.cp_3p) {
      const fit = fitness.cp_3p;
      const ds3 = pts.map(([d]) => d).filter((d) => d >= 5 && d <= 3600);
      lines.push({ name: "CP 3-parameter", type: "line", showSymbol: false, data: ds3.map((d) => [d, model(fit, d)]), lineStyle: { color: c("--tsb"), type: "dotted", width: 1.5 } });
    }
    return {
      grid: { left: 55, right: 20, top: 40, bottom: 45 },
      legend: { top: 0, textStyle: { color: c("--text") } },
      tooltip: { trigger: "axis", valueFormatter: (v: number) => `${num(v)} W` },
      xAxis: { type: "log", logBase: 10, name: "duration", nameLocation: "middle", nameGap: 28, axisLabel: { color: c("--text-muted"), formatter: (v: number) => durationLabel(v) }, splitLine: { lineStyle: { color: c("--border") } } },
      yAxis: { type: "value", name: "W", axisLabel: { color: c("--text-muted") }, splitLine: { lineStyle: { color: c("--border") } } },
      series: lines,
    };
  });

  const today = $derived(fitness?.days.at(-1));
  const tableRows = $derived(
    fitness
      ? fitness.days
          .map((d, i) => [isoDay(dayIndexToDate(fitness!.first_day + i)), num(d.load), num(d.ctl, 1), num(d.atl, 1), num(d.tsb, 1)])
          .filter((_, i, all) => i >= all.length - 60)
      : [],
  );
</script>

<svelte:head><title>Fitness · zerofit</title></svelte:head>

<h1>Fitness</h1>

{#if error}
  <p class="error" role="alert">{error}</p>
{:else if app.activities.length === 0}
  <p>No activities yet. <a href="{base}/upload/">Upload FIT files</a> or load the demo.</p>
{:else if !fitness}
  <p class="muted">Computing…</p>
{:else}
  <section class="card" aria-labelledby="pmc-h">
    <h2 id="pmc-h">Fitness, fatigue and form</h2>
    {#if today}
      <div class="stats">
        <div class="stat"><div class="label">Fitness (CTL)</div><div class="value">{num(today.ctl)}</div></div>
        <div class="stat"><div class="label">Fatigue (ATL)</div><div class="value">{num(today.atl)}</div></div>
        <div class="stat"><div class="label">Form (TSB)</div><div class="value">{num(today.tsb)}</div></div>
      </div>
    {/if}
    <TimeSeries title="Fitness, fatigue and form over time" data={loadData} series={loadSeries} xIsDuration={false} xFormat={(x) => isoDay(new Date(x * 1000))} height={260} />
    {#if app.activities.some((a) => a.demo)}
      <p class="muted small">Demo rides are shown as ridden in the last ten days, so this chart starts there.</p>
    {/if}
    <p class="muted small">
      CTL and ATL are 42- and 7-day exponentially weighted averages of daily training load (TSS, or hrTSS for rides
      without power), using intervals.icu's conventions: decay e<sup>−1/τ</sup>, form after the day's training.
    </p>
    <DataTable caption="Daily training load, last 60 days" headers={["Day", "Load", "CTL", "ATL", "TSB"]} rows={tableRows} />
  </section>

  <section class="card" aria-labelledby="curve-h">
    <h2 id="curve-h">Season power curve</h2>
    {#if fitness.season_curve.length > 0}
      <EChart title="Best power for each duration across all activities, with fitted critical power models" option={curveOption} height={320} />
      <DataTable caption="Season power curve" headers={["Duration", "Watts"]} rows={fitness.season_curve.map(([d, w]) => [durationLabel(d), num(w)])} />
    {:else}
      <p class="muted">No rides with power yet.</p>
    {/if}
  </section>

  <div class="grid">
    <section class="card" aria-labelledby="cp-h">
      <h2 id="cp-h">Critical power</h2>
      {#if fitness.cp_2p}
        <h3>2-parameter (2–20 min)</h3>
        <div class="stats">
          <div class="stat"><div class="label">CP</div><div class="value">{num(fitness.cp_2p.cp)}<span class="unit">± {num(fitness.cp_2p.se_cp)} W</span></div></div>
          <div class="stat"><div class="label">W′</div><div class="value">{num(fitness.cp_2p.w_prime / 1000, 1)}<span class="unit">kJ</span></div></div>
          <div class="stat"><div class="label">R²</div><div class="value">{num(fitness.cp_2p.r_squared, 3)}</div></div>
          <div class="stat"><div class="label">RMSE</div><div class="value">{num(fitness.cp_2p.rmse, 1)}<span class="unit">W</span></div></div>
        </div>
      {/if}
      {#if fitness.cp_3p}
        <h3>3-parameter Morton (10 s–20 min)</h3>
        <div class="stats">
          <div class="stat"><div class="label">CP</div><div class="value">{num(fitness.cp_3p.cp)}<span class="unit">W</span></div></div>
          <div class="stat"><div class="label">W′</div><div class="value">{num(fitness.cp_3p.w_prime / 1000, 1)}<span class="unit">kJ</span></div></div>
          <div class="stat"><div class="label">Pmax</div><div class="value">{num(fitness.cp_3p.p_max)}<span class="unit">W</span></div></div>
          <div class="stat"><div class="label">R²</div><div class="value">{num(fitness.cp_3p.r_squared, 3)}</div></div>
        </div>
      {/if}
      {#if !fitness.cp_2p && !fitness.cp_3p}<p class="muted">Not enough maximal efforts between 2 and 20 minutes.</p>{/if}
    </section>
    <section class="card" aria-labelledby="eftp-h">
      <h2 id="eftp-h">Estimated FTP</h2>
      {#if fitness.eftp}
        <div class="stats">
          <div class="stat"><div class="label">eFTP</div><div class="value" data-testid="eftp">{num(fitness.eftp.eftp)}<span class="unit">W</span></div></div>
          <div class="stat"><div class="label">Settings FTP</div><div class="value">{num(app.settings.ftp)}<span class="unit">W</span></div></div>
        </div>
        <p class="small">
          Your best {duration(fitness.eftp.duration_s)} ({num(fitness.eftp.watts)} W) places you on a power-duration curve with W′
          {num(fitness.eftp.w_prime / 1000, 1)} kJ; eFTP is that curve's one-hour power.
        </p>
      {:else}
        <p class="muted">Needs a ride with power and an effort of at least 3 minutes.</p>
      {/if}
    </section>
  </div>
{/if}

<style>
  .card,
  .grid {
    margin-bottom: 1rem;
  }
  .small {
    font-size: 0.85rem;
  }
  h3 {
    margin-top: 0.75rem;
    font-size: 0.95rem;
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
  }
</style>
