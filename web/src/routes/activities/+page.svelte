<script lang="ts">
  import { base } from "$app/paths";
  import { app, removeActivity } from "$lib/state.svelte";
  import type { ActivityMeta } from "$lib/db";
  import { date, duration, num } from "$lib/format";

  type Key = "date" | "name" | "duration" | "np" | "tss" | "if";
  const columns: { key: Key; label: string; num: boolean; get: (a: ActivityMeta) => number | string | null }[] = [
    { key: "date", label: "Date", num: false, get: (a) => a.startMs },
    { key: "name", label: "Activity", num: false, get: (a) => a.name.toLowerCase() },
    { key: "duration", label: "Duration", num: true, get: (a) => a.summary.moving_time_s },
    { key: "np", label: "NP (W)", num: true, get: (a) => a.summary.normalized_power },
    { key: "tss", label: "TSS", num: true, get: (a) => a.summary.tss ?? a.summary.hr_tss },
    { key: "if", label: "IF", num: true, get: (a) => a.summary.intensity_factor },
  ];

  let sortKey = $state<Key>("date");
  let ascending = $state(false);

  const sorted = $derived.by(() => {
    const col = columns.find((c) => c.key === sortKey) ?? columns[0]!;
    return [...app.activities].sort((a, b) => {
      const x = col.get(a);
      const y = col.get(b);
      // Missing values always sort last.
      if (x == null) return 1;
      if (y == null) return -1;
      const cmp = x < y ? -1 : x > y ? 1 : 0;
      return ascending ? cmp : -cmp;
    });
  });

  function sortBy(key: Key) {
    if (sortKey === key) ascending = !ascending;
    else {
      sortKey = key;
      ascending = key === "name";
    }
  }
</script>

<svelte:head><title>Activities · zerofit</title></svelte:head>

<h1>Activities</h1>

{#if app.activities.length === 0}
  <p>No activities yet. <a href="{base}/upload/">Upload FIT files</a> or load the demo from the overview.</p>
{:else}
  <div class="card table-wrap">
    <table>
      <caption class="visually-hidden">Activities, sortable by column</caption>
      <thead>
        <tr>
          {#each columns as c (c.key)}
            <th
              scope="col"
              class:num={c.num}
              aria-sort={sortKey === c.key ? (ascending ? "ascending" : "descending") : "none"}
            >
              <button class="sort" onclick={() => sortBy(c.key)}>
                {c.label}
                <span aria-hidden="true">{sortKey === c.key ? (ascending ? "▲" : "▼") : ""}</span>
              </button>
            </th>
          {/each}
          <th scope="col"><span class="visually-hidden">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        {#each sorted as a (a.id)}
          <tr>
            <td>{date(a.startMs)}</td>
            <td class="name"><a href="{base}/activity/?id={a.id}">{a.name}</a></td>
            <td class="num">{duration(a.summary.moving_time_s)}</td>
            <td class="num">{num(a.summary.normalized_power)}</td>
            <td class="num">
              {num(a.summary.tss ?? a.summary.hr_tss)}{#if a.summary.tss == null && a.summary.hr_tss != null}<abbr title="Heart-rate TSS (no power data)">*</abbr>{/if}
            </td>
            <td class="num">{num(a.summary.intensity_factor, 2)}</td>
            <td>
              <button class="danger small" onclick={() => removeActivity(a.id)} aria-label="Delete {a.name}">Delete</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <p class="muted note">* hrTSS: rides without a power meter use heart-rate training stress. Duration is moving time.</p>
{/if}

<style>
  .sort {
    border: 0;
    background: none;
    padding: 0.25rem 0;
    min-height: 2rem;
    font-weight: 600;
    color: var(--text-muted);
  }
  th.num .sort {
    margin-left: auto;
  }
  .name {
    white-space: normal;
    min-width: 12rem;
  }
  .small {
    min-height: 2.25rem;
    padding: 0.25rem 0.6rem;
    font-size: 0.85rem;
  }
  .note {
    font-size: 0.85rem;
  }
</style>
