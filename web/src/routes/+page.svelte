<script lang="ts">
  import { base } from "$app/paths";
  import { app, loadDemo } from "$lib/state.svelte";
  import { computeFitness } from "$lib/fitness";
  import { date, duration, num } from "$lib/format";
  import type { Fitness } from "$lib/types";

  let fitness = $state<Fitness | null>(null);
  let loadingDemo = $state(false);

  $effect(() => {
    const list = app.activities;
    if (!app.ready || list.length === 0) {
      fitness = null;
      return;
    }
    computeFitness($state.snapshot(list), app.settings.w_prime).then((f) => (fitness = f));
  });

  const latest = $derived(app.activities[0]);
  const today = $derived(fitness?.days.at(-1));

  async function demo() {
    loadingDemo = true;
    try {
      await loadDemo();
    } finally {
      loadingDemo = false;
    }
  }
</script>

<svelte:head><title>zerofit: ride analysis in your browser</title></svelte:head>

<section class="hero">
  <h1>Ride analysis that never leaves your browser</h1>
  <p class="lead">
    Drop in FIT files from any bike computer. A Rust decoder and analytics engine, compiled to WebAssembly and running in
    a Web Worker, computes normalized power, TSS, power curves, W′ balance and fitness trends on your device. Then plan a
    week of workouts and get a fueling plan for every day of it.
  </p>
  <div class="row">
    <a class="button primary" href="{base}/upload/">Upload FIT files</a>
    <button onclick={demo} disabled={loadingDemo}>{loadingDemo ? "Loading demo…" : "Load demo data"}</button>
    <a class="button" href="{base}/activities/">Browse activities</a>
  </div>
</section>

{#if !app.ready}
  <p class="muted">Loading…</p>
{:else if app.activities.length === 0}
  <div class="card">
    <h2>No activities yet</h2>
    <p>Upload your own FIT files or load the demo: four anonymized real rides and a sample training week.</p>
  </div>
{:else}
  <div class="grid">
    <section class="card" aria-labelledby="form-h">
      <h2 id="form-h">Today's form</h2>
      {#if today}
        <div class="stats">
          <div class="stat"><div class="label">Fitness (CTL)</div><div class="value">{num(today.ctl)}</div></div>
          <div class="stat"><div class="label">Fatigue (ATL)</div><div class="value">{num(today.atl)}</div></div>
          <div class="stat"><div class="label">Form (TSB)</div><div class="value">{num(today.tsb)}</div></div>
        </div>
        <p class="muted small">Across {app.activities.length} activities. <a href="{base}/fitness/">Fitness details</a></p>
        {#if app.activities.some((a) => a.demo)}
          <p class="muted small">Demo rides are shown as ridden in the last ten days; their real dates span three years.</p>
        {/if}
      {:else}
        <p class="muted">Computing…</p>
      {/if}
    </section>

    {#if latest}
      <section class="card" aria-labelledby="latest-h">
        <h2 id="latest-h">Latest ride</h2>
        <p><a href="{base}/activity/?id={latest.id}">{latest.name}</a><br /><span class="muted">{date(latest.startMs)}</span></p>
        <div class="stats">
          <div class="stat"><div class="label">Moving</div><div class="value">{duration(latest.summary.moving_time_s)}</div></div>
          <div class="stat"><div class="label">NP</div><div class="value">{num(latest.summary.normalized_power)}<span class="unit">W</span></div></div>
          <div class="stat"><div class="label">TSS</div><div class="value">{num(latest.summary.tss ?? latest.summary.hr_tss)}</div></div>
        </div>
      </section>
    {/if}

    <section class="card" aria-labelledby="eftp-h">
      <h2 id="eftp-h">Estimated FTP</h2>
      {#if fitness?.eftp}
        <div class="stats">
          <div class="stat"><div class="label">eFTP</div><div class="value">{num(fitness.eftp.eftp)}<span class="unit">W</span></div></div>
          {#if fitness.cp_2p}
            <div class="stat"><div class="label">CP</div><div class="value">{num(fitness.cp_2p.cp)}<span class="unit">W</span></div></div>
            <div class="stat"><div class="label">W′</div><div class="value">{num(fitness.cp_2p.w_prime / 1000, 1)}<span class="unit">kJ</span></div></div>
          {/if}
        </div>
        <p class="muted small">From your best {duration(fitness.eftp.duration_s)} effort ({num(fitness.eftp.watts)} W).</p>
      {:else}
        <p class="muted">Needs rides with power.</p>
      {/if}
    </section>
  </div>
{/if}

<section class="how card">
  <h2>How it works</h2>
  <ol>
    <li><strong>Decode:</strong> a zero-copy, panic-free FIT decoder (<code>zerofit</code>) reads the file's bytes in place.</li>
    <li><strong>Analyze:</strong> <code>zerofit-analytics</code> resamples to 1 Hz and computes every metric, validated against intervals.icu.</li>
    <li><strong>Fuel:</strong> <code>zerofit-fueling</code> turns planned training into carbohydrate and protein targets from published sports-nutrition consensus.</li>
    <li><strong>Run locally:</strong> all three crates are one WebAssembly module in a Web Worker; nothing is uploaded anywhere.</li>
  </ol>
</section>

<style>
  .hero {
    padding: 1.5rem 0 1rem;
  }
  .hero h1 {
    font-size: clamp(1.5rem, 4vw, 2.2rem);
    max-width: 22em;
  }
  .lead {
    max-width: 48em;
    color: var(--text-muted);
    font-size: 1.05rem;
  }
  .grid {
    margin: 1rem 0;
  }
  .small {
    font-size: 0.85rem;
  }
  .how {
    margin-top: 1rem;
  }
  .how ol {
    padding-left: 1.25rem;
    margin: 0;
  }
  .how li {
    margin: 0.35rem 0;
  }
</style>
