<script lang="ts">
  import { base } from "$app/paths";
  import { app } from "$lib/state.svelte";
  import { call } from "$lib/worker/client";
  import { addDays, clock, duration, isoDay, mondayOf, num } from "$lib/format";
  import { plannedLoad, workoutDuration } from "$lib/plan";
  import type { DayPlan, FuelEntry, FuelSessionIn } from "$lib/types";
  import EChart from "$lib/components/EChart.svelte";
  import DataTable from "$lib/components/DataTable.svelte";

  let weekStart = $state(mondayOf(new Date()));
  const days = $derived(Array.from({ length: 7 }, (_, i) => addDays(weekStart, i)));
  let plans = $state<Record<string, DayPlan | string>>({});
  let selected = $state(isoDay(new Date()));

  async function sessionsOn(day: string): Promise<FuelSessionIn[]> {
    const items = app.plan.filter((p) => p.date === day).sort((a, b) => a.start_min - b.start_min);
    return Promise.all(
      items.map(async (p) => {
        const load = await plannedLoad($state.snapshot(p.workout), app.settings.ftp);
        return {
          start_min: p.start_min,
          duration_min: Math.round(workoutDuration(p.workout) / 60),
          intensity_factor: load.intensity_factor ?? 0.65,
          work_kj: load.work_kj,
        };
      }),
    );
  }

  $effect(() => {
    if (!app.ready) return;
    const ds = days.map(isoDay);
    void app.plan.length;
    void app.settings.weight_kg;
    void (async () => {
      const next: Record<string, DayPlan | string> = {};
      for (const day of ds) {
        const sessions = await sessionsOn(day);
        const tomorrow = await sessionsOn(isoDay(addDays(new Date(day + "T00:00"), 1)));
        try {
          next[day] = await call("fuelingDay", [
            {
              body_mass_kg: app.settings.weight_kg,
              ftp_w: app.settings.ftp,
              sessions,
              next_session_start_min: tomorrow[0] ? 1440 + tomorrow[0].start_min : undefined,
            },
          ]);
        } catch (e) {
          next[day] = e instanceof Error ? e.message : String(e);
        }
      }
      plans = next;
      if (!ds.includes(selected)) selected = ds[0] ?? selected;
    })();
  });

  const plan = $derived(typeof plans[selected] === "object" ? (plans[selected] as DayPlan) : null);
  const bandLabel = { Light: "Light", Moderate: "Moderate", High: "High", VeryHigh: "Very high" } as const;

  function what(e: FuelEntry): string {
    switch (e.kind.type) {
      case "meal":
        return { Breakfast: "Breakfast", Lunch: "Lunch", Dinner: "Dinner", EveningSnack: "Evening snack" }[e.kind.meal];
      case "pre_session":
        return `Pre-ride meal (session ${e.kind.session + 1})`;
      case "during_session":
        return `On the bike (session ${e.kind.session + 1})`;
      case "recovery":
        return e.carbs_g > 0 ? `Recovery, hour ${e.kind.hour + 1}` : "Post-ride protein";
    }
  }

  const kindColor = (e: FuelEntry) =>
    e.kind.type === "during_session" ? "--power" : e.kind.type === "pre_session" ? "--tsb" : e.kind.type === "recovery" ? "--hr" : "--altitude";

  const timelineOption = $derived((c: (v: string) => string) => ({
    grid: { left: 45, right: 10, top: 20, bottom: 35 },
    tooltip: { trigger: "item", formatter: (p: { data: { name: string; value: [number, number] } }) => `${clock(Math.round(p.data.value[0] * 60))} ${p.data.name}: ${num(p.data.value[1])} g carbs` },
    xAxis: { type: "value", min: 5, max: Math.max(23, ...(plan?.entries.map((e) => e.time_min / 60 + 1) ?? [])), name: "hour", nameLocation: "middle", nameGap: 22, axisLabel: { color: c("--text-muted"), formatter: (h: number) => `${Math.floor(h) % 24}:00` }, splitLine: { lineStyle: { color: c("--border") } } },
    yAxis: { type: "value", name: "carbs g", axisLabel: { color: c("--text-muted") }, splitLine: { lineStyle: { color: c("--border") } } },
    series: [
      {
        type: "bar",
        barMaxWidth: 14,
        data: (plan?.entries ?? []).map((e) => ({ name: what(e), value: [e.time_min / 60, Math.round(e.carbs_g)], itemStyle: { color: c(kindColor(e)) } })),
      },
    ],
  }));
</script>

<svelte:head><title>Fueling · zerofit</title></svelte:head>

<h1>Fueling</h1>

<aside class="notice disclaimer" role="note" aria-label="Disclaimer">
  <strong>General guidance, not medical or dietary advice.</strong> These targets come from published sports-nutrition
  consensus for healthy athletes (ACSM/AND/DC 2016, IOC/Burke et al. 2011, Jeukendrup 2014). Individual needs vary; if
  you have a medical condition, an eating disorder history, or specific goals, talk to a registered dietitian or doctor.
</aside>

<div class="row week-nav">
  <button onclick={() => (weekStart = addDays(weekStart, -7))} aria-label="Previous week">←</button>
  <h2 class="week-title">Week of {weekStart.toLocaleDateString(undefined, { month: "long", day: "numeric" })}</h2>
  <button onclick={() => (weekStart = addDays(weekStart, 7))} aria-label="Next week">→</button>
  <span class="muted small">Body mass {app.settings.weight_kg} kg · workouts from the <a href="{base}/plan/">plan</a></span>
</div>

<div class="days" role="tablist" aria-label="Days">
  {#each days as d (isoDay(d))}
    {@const p = plans[isoDay(d)]}
    <button
      role="tab"
      class="daytab"
      aria-selected={selected === isoDay(d)}
      onclick={() => (selected = isoDay(d))}
    >
      <span class="dn">{d.toLocaleDateString(undefined, { weekday: "short", day: "numeric" })}</span>
      {#if typeof p === "object"}
        <span class="gk">{num(p.carbs_g_per_kg, 1)} g/kg</span>
        <span class="band">{bandLabel[p.band]}</span>
      {:else}
        <span class="muted">…</span>
      {/if}
    </button>
  {/each}
</div>

{#if typeof plans[selected] === "string"}
  <p class="error" role="alert">{plans[selected]}</p>
{:else if plan}
  <div class="card" role="tabpanel" aria-labelledby="day-h">
    <h2 id="day-h">{new Date(selected + "T00:00").toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}</h2>
    <div class="stats">
      <div class="stat"><div class="label">Load band</div><div class="value">{bandLabel[plan.band]}</div></div>
      <div class="stat"><div class="label">Carbohydrate</div><div class="value" data-testid="carbs-gkg">{num(plan.carbs_g_per_kg, 1)}<span class="unit">g/kg</span></div></div>
      <div class="stat"><div class="label">Carbs total</div><div class="value">{num(plan.carbs_planned_g)}<span class="unit">g</span></div></div>
      <div class="stat"><div class="label">Protein</div><div class="value">{num(plan.protein_g_per_kg, 1)}<span class="unit">g/kg</span></div></div>
      <div class="stat"><div class="label">Protein total</div><div class="value">{num(plan.protein_g)}<span class="unit">g</span></div></div>
    </div>
    {#if plan.session_feeds_exceed_target}
      <p class="small">Session fueling alone exceeds the daily target: the in-ride and recovery rules take priority today.</p>
    {/if}

    {#each plan.sessions as s, i (i)}
      <div class="session">
        <h3>Session {i + 1}: {clock(s.start_min)}, {duration(s.duration_min * 60)}</h3>
        <ul>
          <li><strong>Before:</strong> {num(s.pre.carbs_g_per_kg, 1)} g/kg ({num(s.pre.carbs_g_per_kg * app.settings.weight_kg)} g) carbohydrate, {num(s.pre.hours_before, 1)} h before the start.</li>
          <li>
            <strong>During:</strong>
            {#if s.during.carbs_g_per_hour > 0}
              {num(s.during.carbs_g_per_hour)} g carbohydrate per hour ({num(s.during_total_g)} g total), every {s.during.feed_interval_min} min{#if s.during.multiple_transportable}, from a glucose + fructose mix (above 60 g/h a single sugar's gut transporter saturates){/if}.
            {:else}
              none needed for a session under an hour; water is enough.
            {/if}
          </li>
          <li>
            <strong>After:</strong>
            {#if s.recovery.hours > 0}
              {num(s.recovery.carbs_g_per_kg_per_hour, 1)} g/kg/h carbohydrate for {s.recovery.hours} h, because the next session is less than 24 h away, plus {num(s.recovery.protein_g_per_kg * app.settings.weight_kg)} g protein.
            {:else}
              {num(s.recovery.protein_g_per_kg * app.settings.weight_kg)} g protein; regular meals restore glycogen before the next session.
            {/if}
          </li>
        </ul>
      </div>
    {:else}
      <p class="muted">Rest day: no sessions planned.</p>
    {/each}

    <h3>Timeline</h3>
    <EChart title="Carbohydrate through the day, coloured by meal, pre-ride, on-bike and recovery feeds" option={timelineOption} height={220} />
    <div class="table-wrap">
      <table data-testid="fuel-table">
        <caption class="visually-hidden">What to eat when</caption>
        <thead><tr><th scope="col">Time</th><th scope="col">What</th><th scope="col" class="num">Carbs (g)</th><th scope="col" class="num">Protein (g)</th></tr></thead>
        <tbody>
          {#each plan.entries as e, i (i)}
            <tr><td>{clock(e.time_min)}</td><td>{what(e)}</td><td class="num">{num(e.carbs_g)}</td><td class="num">{num(e.protein_g)}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
    <DataTable caption="Timeline chart data" headers={["Time", "Carbs (g)"]} rows={plan.entries.map((e) => [clock(e.time_min), num(e.carbs_g)])} />
  </div>
{:else}
  <p class="muted">Planning…</p>
{/if}

<style>
  .disclaimer {
    margin-bottom: 1rem;
  }
  .week-nav {
    margin-bottom: 0.75rem;
  }
  .week-title {
    margin: 0;
    font-size: 1.1rem;
  }
  .days {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 0.4rem;
    margin-bottom: 1rem;
  }
  @media (max-width: 700px) {
    .days {
      grid-template-columns: repeat(4, minmax(0, 1fr));
    }
  }
  .daytab {
    display: grid;
    justify-items: start;
    gap: 0.05rem;
    padding: 0.4rem 0.5rem;
  }
  .daytab[aria-selected="true"] {
    outline: 2px solid var(--accent);
    background: var(--surface-2);
  }
  .dn {
    font-weight: 600;
  }
  .gk {
    font-variant-numeric: tabular-nums;
  }
  .band {
    font-size: 0.75rem;
    color: var(--text-muted);
  }
  .session {
    margin-top: 1rem;
  }
  .session ul {
    margin: 0.25rem 0 0;
    padding-left: 1.25rem;
  }
  .small {
    font-size: 0.85rem;
  }
  h3 {
    margin-top: 1.25rem;
  }
  .error {
    color: var(--danger);
  }
</style>
