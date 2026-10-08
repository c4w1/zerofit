<script lang="ts">
  import { app, savePlan } from "$lib/state.svelte";
  import { call } from "$lib/worker/client";
  import type { PlanItem } from "$lib/demo";
  import { addDays, clock, download, duration, isoDay, mondayOf, num, parseIsoDay, weekdayShort, FIT_EPOCH_OFFSET_S } from "$lib/format";
  import { plannedLoad, profilePoints, workoutDuration } from "$lib/plan";
  import type { PlannedLoad, StepKind, Workout, WorkoutStep } from "$lib/types";
  import EChart from "$lib/components/EChart.svelte";

  let weekStart = $state(mondayOf(new Date()));
  const days = $derived(Array.from({ length: 7 }, (_, i) => addDays(weekStart, i)));
  let selectedId = $state<string | null>(null);
  const selected = $derived(app.plan.find((p) => p.id === selectedId) ?? null);

  // Planned load of every visible item (and the editor's live value).
  let loads = $state<Record<string, PlannedLoad>>({});
  $effect(() => {
    const ftp = app.settings.ftp;
    for (const item of app.plan) {
      const w = $state.snapshot(item.workout);
      plannedLoad(w, ftp).then((l) => (loads[item.id] = l)).catch(() => {});
    }
  });

  const itemsOn = (d: Date) => app.plan.filter((p) => p.date === isoDay(d)).sort((a, b) => a.start_min - b.start_min);
  const weekTss = $derived(
    days.flatMap((d) => itemsOn(d)).reduce((t, p) => t + (loads[p.id]?.tss ?? 0), 0),
  );

  // Save on every edit, not debounced: a pending timer is lost on reload.
  function persist() {
    savePlan().catch((e) => (app.error = `Could not save the plan: ${e instanceof Error ? e.message : String(e)}`));
  }

  function newWorkout(d: Date) {
    const item: PlanItem = {
      id: crypto.randomUUID(),
      date: isoDay(d),
      start_min: 18 * 60,
      workout: {
        name: "New workout",
        steps: [
          { duration_s: 600, kind: "warmup", target: { type: "ramp", from: 50, to: 75 } },
          { duration_s: 1200, kind: "active", target: { type: "steady", pct: 88 } },
          { duration_s: 600, kind: "cooldown", target: { type: "ramp", from: 60, to: 40 } },
        ],
      },
    };
    app.plan.push(item);
    selectedId = item.id;
    persist();
  }

  function removeItem(id: string) {
    app.plan = app.plan.filter((p) => p.id !== id);
    if (selectedId === id) selectedId = null;
    persist();
  }

  // ---- step editing (operates on the selected workout in place) ----
  function edit(f: (w: Workout) => void) {
    if (!selected) return;
    f(selected.workout);
    persist();
  }
  const addSteady = () => edit((w) => w.steps.push({ duration_s: 300, kind: "active", target: { type: "steady", pct: 75 } }));
  const addRamp = () => edit((w) => w.steps.push({ duration_s: 600, kind: "active", target: { type: "ramp", from: 60, to: 90 } }));
  let reps = $state(4);
  let workMin = $state(4);
  let workPct = $state(115);
  let restMin = $state(4);
  let restPct = $state(50);
  const addSet = () =>
    edit((w) => {
      for (let i = 0; i < reps; i++) {
        w.steps.push({ duration_s: Math.round(workMin * 60), kind: "active", target: { type: "steady", pct: workPct } });
        w.steps.push({ duration_s: Math.round(restMin * 60), kind: "recovery", target: { type: "steady", pct: restPct } });
      }
    });
  const move = (i: number, by: number) =>
    edit((w) => {
      const j = i + by;
      if (j < 0 || j >= w.steps.length) return;
      const [s] = w.steps.splice(i, 1);
      if (s) w.steps.splice(j, 0, s);
    });
  const removeStep = (i: number) => edit((w) => w.steps.splice(i, 1));
  function setTargetType(step: WorkoutStep, type: "steady" | "ramp") {
    const now = step.target.type === "steady" ? step.target.pct : (step.target.from + step.target.to) / 2;
    step.target = type === "steady" ? { type, pct: now } : { type, from: now, to: now };
    persist();
  }

  const minutes = (s: WorkoutStep) => +(s.duration_s / 60).toFixed(2);

  async function exportZwo() {
    if (!selected) return;
    const xml = await call("exportZwo", [$state.snapshot(selected.workout)]);
    download(`${fileName(selected.workout.name)}.zwo`, xml, "application/xml");
  }
  async function exportFit() {
    if (!selected) return;
    const fitTime = Math.floor(Date.now() / 1000) - FIT_EPOCH_OFFSET_S;
    const bytes = await call("exportFit", [$state.snapshot(selected.workout), fitTime]);
    download(`${fileName(selected.workout.name)}.fit`, bytes as BlobPart, "application/vnd.ant.fit");
  }
  const fileName = (n: string) => n.replace(/[^\w.-]+/g, "_").slice(0, 60) || "workout";

  const shapeOption = $derived((c: (v: string) => string) => ({
    grid: { left: 45, right: 10, top: 10, bottom: 30 },
    xAxis: { type: "value", max: selected ? workoutDuration(selected.workout) : 1, axisLabel: { color: c("--text-muted"), formatter: (v: number) => duration(v) }, splitLine: { show: false } },
    yAxis: { type: "value", name: "% FTP", min: 0, axisLabel: { color: c("--text-muted") }, splitLine: { lineStyle: { color: c("--border") } } },
    series: [
      {
        type: "line",
        showSymbol: false,
        data: selected ? profilePoints(selected.workout) : [],
        lineStyle: { color: c("--power"), width: 1.5 },
        areaStyle: { color: c("--power"), opacity: 0.25 },
        markLine: { silent: true, symbol: "none", data: [{ yAxis: 100 }], lineStyle: { color: c("--text-muted"), type: "dashed" }, label: { formatter: "FTP", color: c("--text-muted") } },
      },
    ],
  }));
</script>

<svelte:head><title>Plan · zerofit</title></svelte:head>

<h1>Plan</h1>

<div class="row week-nav">
  <button onclick={() => (weekStart = addDays(weekStart, -7))} aria-label="Previous week">←</button>
  <h2 class="week-title" aria-live="polite">Week of {weekStart.toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" })}</h2>
  <button onclick={() => (weekStart = addDays(weekStart, 7))} aria-label="Next week">→</button>
  <button onclick={() => (weekStart = mondayOf(new Date()))}>This week</button>
  <span class="week-tss">Planned TSS this week: <strong data-testid="week-tss">{num(weekTss)}</strong></span>
</div>

<ol class="calendar" aria-label="Days of the week">
  {#each days as d (isoDay(d))}
    <li class="day card" class:today={isoDay(d) === isoDay(new Date())}>
      <h3><span>{weekdayShort(d)}</span> <span class="muted">{d.getDate()}</span></h3>
      {#each itemsOn(d) as item (item.id)}
        <button class="item" class:active={item.id === selectedId} onclick={() => (selectedId = item.id)} aria-pressed={item.id === selectedId}>
          <span class="t">{clock(item.start_min)}</span>
          <span class="n">{item.workout.name}</span>
          <span class="m">{duration(workoutDuration(item.workout))} · TSS {num(loads[item.id]?.tss)}</span>
        </button>
      {:else}
        <p class="rest muted">Rest</p>
      {/each}
      <button class="add" onclick={() => newWorkout(d)} aria-label="Add workout on {d.toLocaleDateString(undefined, { weekday: 'long' })}">+ Add</button>
    </li>
  {/each}
</ol>

{#if selected}
  {@const load = loads[selected.id]}
  <section class="card editor" aria-labelledby="builder-h">
    <h2 id="builder-h">Workout builder</h2>
    <div class="row fields">
      <label>Name <input bind:value={selected.workout.name} oninput={persist} data-testid="workout-name" /></label>
      <label>
        Day
        <select bind:value={selected.date} onchange={persist}>
          {#each days as d (isoDay(d))}<option value={isoDay(d)}>{d.toLocaleDateString(undefined, { weekday: "long", day: "numeric" })}</option>{/each}
          {#if !days.some((d) => isoDay(d) === selected.date)}<option value={selected.date}>{parseIsoDay(selected.date).toLocaleDateString()}</option>{/if}
        </select>
      </label>
      <label>
        Start
        <input
          type="time"
          value={clock(selected.start_min)}
          onchange={(e) => {
            const [h, m] = (e.currentTarget as HTMLInputElement).value.split(":").map(Number);
            selected.start_min = (h ?? 0) * 60 + (m ?? 0);
            persist();
          }}
        />
      </label>
    </div>

    <div class="stats planned" aria-live="polite">
      <div class="stat"><div class="label">Duration</div><div class="value">{duration(workoutDuration(selected.workout))}</div></div>
      <div class="stat"><div class="label">Planned TSS</div><div class="value" data-testid="planned-tss">{num(load?.tss)}</div></div>
      <div class="stat"><div class="label">IF</div><div class="value">{num(load?.intensity_factor, 2)}</div></div>
      <div class="stat"><div class="label">NP</div><div class="value">{num(load?.normalized_power)}<span class="unit">W</span></div></div>
      <div class="stat"><div class="label">Work</div><div class="value">{num(load?.work_kj)}<span class="unit">kJ</span></div></div>
    </div>
    <p class="muted small">At FTP {app.settings.ftp} W. Computed by the same Rust NP/TSS code as recorded rides.</p>

    <EChart title="Workout power profile in percent of FTP" option={shapeOption} height={170} />

    <div class="table-wrap">
      <table class="steps">
        <caption class="visually-hidden">Workout steps</caption>
        <thead>
          <tr><th scope="col">#</th><th scope="col">Type</th><th scope="col">Minutes</th><th scope="col">Target</th><th scope="col">% FTP</th><th scope="col"><span class="visually-hidden">Actions</span></th></tr>
        </thead>
        <tbody>
          {#each selected.workout.steps as step, i (i)}
            <tr>
              <td>{i + 1}</td>
              <td>
                <select bind:value={step.kind} onchange={persist} aria-label="Step {i + 1} type">
                  {#each ["warmup", "active", "recovery", "cooldown"] as k (k)}<option value={k as StepKind}>{k}</option>{/each}
                </select>
              </td>
              <td>
                <input
                  type="number"
                  min="0.25"
                  step="0.25"
                  value={minutes(step)}
                  aria-label="Step {i + 1} minutes"
                  oninput={(e) => {
                    const v = Number((e.currentTarget as HTMLInputElement).value);
                    if (v > 0) {
                      step.duration_s = Math.round(v * 60);
                      persist();
                    }
                  }}
                />
              </td>
              <td>
                <select value={step.target.type} onchange={(e) => setTargetType(step, (e.currentTarget as HTMLSelectElement).value as "steady" | "ramp")} aria-label="Step {i + 1} target">
                  <option value="steady">steady</option>
                  <option value="ramp">ramp</option>
                </select>
              </td>
              <td class="pct">
                {#if step.target.type === "steady"}
                  <input type="number" min="0" max="300" bind:value={step.target.pct} oninput={persist} aria-label="Step {i + 1} percent of FTP" />
                {:else}
                  <input type="number" min="0" max="300" bind:value={step.target.from} oninput={persist} aria-label="Step {i + 1} start percent of FTP" />
                  →
                  <input type="number" min="0" max="300" bind:value={step.target.to} oninput={persist} aria-label="Step {i + 1} end percent of FTP" />
                {/if}
              </td>
              <td class="actions">
                <button onclick={() => move(i, -1)} disabled={i === 0} aria-label="Move step {i + 1} up">↑</button>
                <button onclick={() => move(i, 1)} disabled={i === selected.workout.steps.length - 1} aria-label="Move step {i + 1} down">↓</button>
                <button onclick={() => removeStep(i)} aria-label="Delete step {i + 1}">✕</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="row adders">
      <button onclick={addSteady}>+ Steady step</button>
      <button onclick={addRamp}>+ Ramp</button>
    </div>
    <fieldset class="set">
      <legend>Add an interval set</legend>
      <div class="row">
        <label>Repeats <input type="number" min="1" max="30" bind:value={reps} /></label>
        <label>Work min <input type="number" min="0.25" step="0.25" bind:value={workMin} /></label>
        <label>Work % <input type="number" min="0" max="300" bind:value={workPct} /></label>
        <label>Rest min <input type="number" min="0.25" step="0.25" bind:value={restMin} /></label>
        <label>Rest % <input type="number" min="0" max="300" bind:value={restPct} /></label>
        <button onclick={addSet} data-testid="add-set">Add {reps} × {workMin} min</button>
      </div>
    </fieldset>

    <div class="row exports">
      <button class="primary" onclick={exportZwo}>Download .zwo (Zwift)</button>
      <button class="primary" onclick={exportFit}>Download .fit workout</button>
      <button class="danger" onclick={() => removeItem(selected.id)}>Delete workout</button>
    </div>
  </section>
{:else}
  <p class="muted">Select a workout to edit it, or add one to a day.</p>
{/if}

<style>
  .week-nav {
    margin-bottom: 0.75rem;
  }
  .week-title {
    margin: 0;
    font-size: 1.1rem;
  }
  .week-tss {
    margin-left: auto;
  }
  .calendar {
    list-style: none;
    padding: 0;
    margin: 0 0 1rem;
    display: grid;
    gap: 0.5rem;
    grid-template-columns: repeat(7, minmax(0, 1fr));
  }
  @media (max-width: 900px) {
    .calendar {
      grid-template-columns: 1fr;
    }
  }
  .day {
    /* Room for one workout, so days don't grow when the plan loads. */
    min-height: 10rem;
    padding: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .day.today {
    border-color: var(--accent);
  }
  .day h3 {
    margin: 0;
    font-size: 0.95rem;
  }
  .item {
    display: grid;
    text-align: left;
    gap: 0.1rem;
    padding: 0.4rem 0.5rem;
    width: 100%;
    background: var(--surface-2);
  }
  .item.active {
    outline: 2px solid var(--accent);
  }
  .item .t,
  .item .m {
    font-size: 0.8rem;
    color: var(--text-muted);
  }
  .item .n {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .rest {
    margin: 0;
    font-size: 0.85rem;
  }
  .add {
    font-size: 0.85rem;
    min-height: 2.25rem;
    margin-top: auto;
  }
  .editor {
    display: grid;
    gap: 0.75rem;
  }
  .fields label {
    display: grid;
    gap: 0.2rem;
  }
  .steps input[type="number"] {
    width: 5.5rem;
  }
  .pct {
    white-space: nowrap;
  }
  .actions button {
    min-height: 2.25rem;
    padding: 0.2rem 0.55rem;
  }
  .set {
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .set label {
    display: grid;
    gap: 0.2rem;
  }
  .set input {
    width: 6rem;
  }
  .small {
    font-size: 0.85rem;
    margin: 0;
  }
</style>
