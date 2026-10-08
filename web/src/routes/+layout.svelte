<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { base } from "$app/paths";
  import { page } from "$app/state";
  import { app, init } from "$lib/state.svelte";
  import { currentThemeChoice, setTheme, startThemeTracking } from "$lib/theme.svelte";

  let { children } = $props();

  const links = [
    { href: "/", label: "Overview" },
    { href: "/upload/", label: "Upload" },
    { href: "/activities/", label: "Activities" },
    { href: "/fitness/", label: "Fitness" },
    { href: "/plan/", label: "Plan" },
    { href: "/fueling/", label: "Fueling" },
    { href: "/settings/", label: "Settings" },
  ];

  let themeChoice = $state<"light" | "dark" | "system">("system");

  onMount(() => {
    startThemeTracking();
    themeChoice = currentThemeChoice();
    void init();
  });

  const current = (href: string) => {
    const path = page.url.pathname.slice(base.length) || "/";
    if (href === "/") return path === "/";
    return path.startsWith(href) || (href === "/activities/" && path.startsWith("/activity"));
  };
</script>

<a class="skip" href="#main">Skip to content</a>

<header>
  <div class="bar">
    <a class="brand" href="{base}/">
      <img src="{base}/favicon.svg" alt="" width="28" height="28" />
      <span>zerofit</span>
    </a>
    <span class="privacy" title="No server, no accounts, no tracking">Runs in your browser · data stays here</span>
    <label class="theme">
      <span class="visually-hidden">Colour theme</span>
      <select
        bind:value={themeChoice}
        onchange={() => setTheme(themeChoice)}
        aria-label="Colour theme"
      >
        <option value="system">System theme</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  </div>
  <nav aria-label="Main">
    <ul>
      {#each links as l (l.href)}
        <li>
          <a href="{base}{l.href}" aria-current={current(l.href) ? "page" : undefined}>{l.label}</a>
        </li>
      {/each}
    </ul>
  </nav>
</header>

<div class="status" role="status" aria-live="polite">
  {#if app.status}<span class="spinner" aria-hidden="true"></span>{app.status}{/if}
</div>
{#if app.error}
  <div class="error" role="alert">{app.error}</div>
{/if}

<main id="main" tabindex="-1">
  {@render children()}
</main>

<footer>
  <p>
    <strong>Your data never leaves this browser.</strong> FIT files are decoded and analyzed by Rust compiled to
    WebAssembly, running in a Web Worker on your device, and stored only in this browser (IndexedDB). There is no
    server, no account and no analytics or tracking.
  </p>
  <p class="muted">
    zerofit {app.wasmVersion ? `· WASM v${app.wasmVersion} loaded in ${app.wasmLoadMs.toFixed(0)} ms` : ""} ·
    <a href="https://github.com/c4w1/zerofit">source code</a> · MIT/Apache-2.0
  </p>
</footer>

<style>
  .skip {
    position: absolute;
    left: -9999px;
    top: 0.5rem;
    z-index: 10;
    background: var(--surface);
    padding: 0.5rem 1rem;
    border-radius: 6px;
  }
  .skip:focus {
    left: 0.5rem;
  }
  header {
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    z-index: 5;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    max-width: 1200px;
    margin: 0 auto;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-weight: 700;
    font-size: 1.15rem;
    color: var(--text);
    text-decoration: none;
  }
  .privacy {
    font-size: 0.8rem;
    color: var(--ok);
    border: 1px solid currentColor;
    border-radius: 999px;
    padding: 0.1rem 0.6rem;
  }
  .theme {
    margin-left: auto;
  }
  .theme select {
    min-height: 2.25rem;
    font-size: 0.9rem;
  }
  nav ul {
    list-style: none;
    margin: 0 auto;
    padding: 0 0.5rem;
    display: flex;
    gap: 0.25rem;
    max-width: 1200px;
    overflow-x: auto;
    scrollbar-width: thin;
  }
  nav a {
    display: block;
    padding: 0.6rem 0.75rem;
    color: var(--text-muted);
    text-decoration: none;
    border-bottom: 3px solid transparent;
    white-space: nowrap;
  }
  nav a:hover {
    color: var(--text);
  }
  nav a[aria-current="page"] {
    color: var(--text);
    border-bottom-color: var(--accent);
    font-weight: 600;
  }
  .status {
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 1rem;
    min-height: 1.75rem;
    font-size: 0.9rem;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .spinner {
    width: 0.9rem;
    height: 0.9rem;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }
  .error {
    max-width: 1200px;
    margin: 0 auto 1rem;
    padding: 0.75rem 1rem;
    color: var(--danger);
    border: 1px solid var(--danger);
    border-radius: var(--radius);
  }
  main {
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 1rem 2rem;
    outline: none;
  }
  footer {
    max-width: 1200px;
    margin: 0 auto;
    padding: 1.5rem 1rem 2.5rem;
    border-top: 1px solid var(--border);
    font-size: 0.9rem;
  }
  @media (max-width: 560px) {
    .privacy {
      display: none;
    }
  }
</style>
