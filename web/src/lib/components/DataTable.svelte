<script lang="ts">
  // The accessible alternative to a chart: the same data as a table,
  // behind a disclosure.
  interface Props {
    caption: string;
    headers: string[];
    rows: (string | number)[][];
  }
  let { caption, headers, rows }: Props = $props();
</script>

<details>
  <summary>Show data as a table</summary>
  <div class="table-wrap">
    <table>
      <caption class="visually-hidden">{caption}</caption>
      <thead>
        <tr>
          {#each headers as h, i (i)}<th scope="col" class:num={i > 0}>{h}</th>{/each}
        </tr>
      </thead>
      <tbody>
        {#each rows as row, r (r)}
          <tr>
            {#each row as cell, i (i)}<td class:num={i > 0}>{cell}</td>{/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</details>

<style>
  details {
    margin-top: 0.5rem;
    font-size: 0.9rem;
  }
  summary {
    cursor: pointer;
    color: var(--accent);
    min-height: 2.25rem;
    display: flex;
    align-items: center;
  }
  .table-wrap {
    max-height: 18rem;
    overflow: auto;
  }
</style>
