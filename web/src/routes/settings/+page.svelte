<script lang="ts">
  import { app, clearAllData, loadDemo, saveSettings } from "$lib/state.svelte";
  import { DEFAULT_SETTINGS } from "$lib/db";
  import type { AthleteSettings } from "$lib/types";

  // Edit a copy; nothing changes until Save.
  let form = $state<AthleteSettings>(structuredClone(DEFAULT_SETTINGS));
  let powerZones = $state("");
  let hrZones = $state("");
  let message = $state("");
  let saving = $state(false);
  let loaded = false;

  $effect(() => {
    if (!app.ready || loaded) return;
    loaded = true;
    form = $state.snapshot(app.settings);
    powerZones = form.power_zones.map((z) => Math.round(z * 100)).join(", ");
    hrZones = form.hr_zones.map((z) => Math.round(z * 100)).join(", ");
  });

  function parseZones(s: string): number[] | null {
    const v = s.split(/[,\s]+/).filter(Boolean).map(Number);
    if (v.length === 0 || v.some((x) => !Number.isFinite(x) || x <= 0)) return null;
    for (let i = 1; i < v.length; i++) if (v[i]! <= v[i - 1]!) return null;
    return v.map((x) => x / 100);
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    const pz = parseZones(powerZones);
    const hz = parseZones(hrZones);
    if (!pz || !hz) {
      message = "Zones must be increasing positive percentages, separated by commas.";
      return;
    }
    if (!(form.resting_hr < form.lthr && form.lthr < form.max_hr)) {
      message = "Heart rates must satisfy resting < threshold < maximum.";
      return;
    }
    saving = true;
    message = "";
    try {
      await saveSettings({ ...form, power_zones: pz, hr_zones: hz, cp: form.cp || undefined, w_prime: form.w_prime || undefined });
      message = `Saved. ${app.activities.length} activities re-analyzed with the new settings.`;
    } catch (err) {
      message = `Could not save: ${err instanceof Error ? err.message : String(err)}`;
    } finally {
      saving = false;
    }
  }

  async function wipe() {
    if (!confirm("Delete every activity, the plan and your settings from this browser? This cannot be undone.")) return;
    await clearAllData();
    loaded = false;
    message = "All data deleted from this browser.";
  }
</script>

<svelte:head><title>Settings · zerofit</title></svelte:head>

<h1>Settings</h1>

<form class="card" onsubmit={save} aria-describedby="settings-help">
  <p id="settings-help" class="muted">Used for IF, TSS, zones, hrTSS, W′ balance and fueling targets. Saving re-analyzes every stored ride.</p>

  <fieldset>
    <legend>Power</legend>
    <div class="fields">
      <label>FTP (W) <input type="number" min="50" max="600" required bind:value={form.ftp} data-testid="ftp" /></label>
      <label>Critical power (W, optional) <input type="number" min="0" max="700" bind:value={form.cp} placeholder="uses FTP" /></label>
      <label>W′ (J, optional) <input type="number" min="0" max="80000" step="500" bind:value={form.w_prime} placeholder="20000" /></label>
      <label>Body weight (kg) <input type="number" min="30" max="200" step="0.1" required bind:value={form.weight_kg} /></label>
    </div>
  </fieldset>

  <fieldset>
    <legend>Heart rate</legend>
    <div class="fields">
      <label>Threshold (LTHR, bpm) <input type="number" min="80" max="220" required bind:value={form.lthr} /></label>
      <label>Maximum (bpm) <input type="number" min="100" max="230" required bind:value={form.max_hr} /></label>
      <label>Resting (bpm) <input type="number" min="25" max="110" required bind:value={form.resting_hr} /></label>
      <label>
        hrTSS coefficients
        <select bind:value={form.trimp}>
          <option value="male">Banister, male (0.64, 1.92)</option>
          <option value="female">Banister, female (0.86, 1.67)</option>
        </select>
      </label>
    </div>
  </fieldset>

  <fieldset>
    <legend>Zones (upper bound of each zone but the last, in %)</legend>
    <div class="fields wide">
      <label>Power zones, % of FTP <input bind:value={powerZones} inputmode="decimal" /></label>
      <label>Heart-rate zones, % of LTHR <input bind:value={hrZones} inputmode="decimal" /></label>
    </div>
    <p class="muted small">Defaults: Coggan 55, 75, 90, 105, 120, 150 and Friel 81, 89, 93, 99, 102, 106 (seven zones each).</p>
  </fieldset>

  <div class="row">
    <button class="primary" type="submit" disabled={saving}>{saving ? "Saving…" : "Save settings"}</button>
    <span role="status" aria-live="polite">{message}</span>
  </div>
</form>

<section class="card data" aria-labelledby="data-h">
  <h2 id="data-h">Your data</h2>
  <p>
    {app.activities.length} activities and {app.plan.length} planned workouts are stored in this browser's IndexedDB. They are never
    sent anywhere; clearing your browser's site data removes them.
  </p>
  <div class="row">
    <button onclick={() => loadDemo()}>Load demo data</button>
    <button class="danger" onclick={wipe}>Delete all data</button>
  </div>
</section>

<style>
  form,
  .data {
    margin-bottom: 1rem;
    display: grid;
    gap: 1rem;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.75rem 1rem 1rem;
  }
  legend {
    font-weight: 600;
    padding: 0 0.3rem;
  }
  .fields {
    display: grid;
    gap: 0.75rem;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 13rem), 1fr));
  }
  .fields.wide {
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 20rem), 1fr));
  }
  .fields label {
    display: grid;
    gap: 0.25rem;
  }
  .small {
    font-size: 0.85rem;
  }
</style>
