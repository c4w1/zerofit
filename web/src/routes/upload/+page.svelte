<script lang="ts">
  import { base } from "$app/paths";
  import { addFiles, app, type UploadResult } from "$lib/state.svelte";

  let dragging = $state(false);
  let busy = $state(false);
  let results = $state<UploadResult[]>([]);
  let input: HTMLInputElement;

  async function handle(files: FileList | File[] | null) {
    const list = Array.from(files ?? []);
    if (list.length === 0) return;
    busy = true;
    try {
      const loaded = await Promise.all(list.map(async (f) => ({ name: f.name, bytes: await f.arrayBuffer() })));
      results = await addFiles(loaded);
    } finally {
      busy = false;
      if (input) input.value = "";
    }
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    void handle(e.dataTransfer?.files ?? null);
  }
</script>

<svelte:head><title>Upload · zerofit</title></svelte:head>

<h1>Upload FIT files</h1>
<p class="muted">
  Files are read and analyzed on this device by the WebAssembly module, then stored in this browser only. Nothing is
  uploaded to a server: "upload" here means "load into the app".
</p>

<label
  class="drop"
  class:dragging
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={onDrop}
>
  <input
    bind:this={input}
    type="file"
    accept=".fit,.FIT,application/vnd.ant.fit"
    multiple
    disabled={busy}
    onchange={(e) => handle((e.currentTarget as HTMLInputElement).files)}
    data-testid="file-input"
  />
  <span class="big">{busy ? "Analyzing…" : "Drop FIT files here"}</span>
  <span class="muted">or press to choose files (several at once is fine)</span>
</label>

{#if results.length > 0}
  <section class="card results" aria-labelledby="results-h">
    <h2 id="results-h">Results</h2>
    <ul>
      {#each results as r (r.name)}
        <li class:ok={r.ok} class:fail={!r.ok}>
          <span class="name">{r.name}</span>:
          {#if r.ok && r.id}
            {r.message}. <a href="{base}/activity/?id={r.id}">Open activity</a>
          {:else}
            could not be read: {r.message}
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<p class="muted">
  {app.activities.length} activities stored. <a href="{base}/activities/">See all</a>
</p>

<style>
  .drop {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    min-height: 12rem;
    border: 2px dashed var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    cursor: pointer;
    text-align: center;
    padding: 1.5rem;
    margin: 1rem 0;
    color: var(--text);
  }
  .drop:focus-within {
    outline: 3px solid var(--focus);
    outline-offset: 2px;
  }
  .drop.dragging {
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .drop input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }
  .big {
    font-size: 1.25rem;
    font-weight: 600;
  }
  .results ul {
    margin: 0;
    padding-left: 1.25rem;
  }
  .fail {
    color: var(--danger);
  }
  .name {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
</style>
