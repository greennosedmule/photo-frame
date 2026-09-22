<script lang="ts">
  // The management view: a pointer-and-keyboard tool, deliberately not the
  // frame's layout. Reads and tagging are open; date edits, deletes, uploads
  // and exports go through the login form.
  import { onMount } from 'svelte';
  import { uploadFiles } from '../lib/api';
  import { withAuth } from '../lib/auth.svelte';
  import { library, refreshManifest, startPolling } from '../lib/state.svelte';
  import Grid from './Grid.svelte';
  import Inspector from './Inspector.svelte';
  import Login from './Login.svelte';
  import StatusPanel from './StatusPanel.svelte';

  let selected = $state(new Set<string>());
  let fileInput: HTMLInputElement | undefined = $state();
  let dragDepth = $state(0);
  let upload = $state<{ text: string; error: boolean } | undefined>();

  /** Newest first, by effective date. */
  const photos = $derived(
    [...(library.manifest?.photos ?? [])].sort(
      (a, b) => b.effective_date.localeCompare(a.effective_date) || a.hash.localeCompare(b.hash),
    ),
  );
  const selection = $derived(photos.filter((p) => selected.has(p.hash)));

  // Drop selections for photographs that no longer exist (deleted elsewhere).
  $effect(() => {
    const live = new Set(photos.map((p) => p.hash));
    if ([...selected].some((h) => !live.has(h))) selected = new Set([...selected].filter((h) => live.has(h)));
  });

  async function send(files: File[]) {
    if (files.length === 0) return;
    upload = { text: `Uploading ${files.length} file${files.length > 1 ? 's' : ''}…`, error: false };
    try {
      const n = await withAuth(() =>
        uploadFiles(files, (f) => (upload = { text: `Uploading ${files.length} file${files.length > 1 ? 's' : ''}… ${Math.round(f * 100)}%`, error: false })),
      );
      // The response does not wait for indexing; the status panel shows the count rise.
      upload = { text: `${n} file${n === 1 ? '' : 's'} received. They will appear once indexed.`, error: false };
      setTimeout(() => (upload = undefined), 6000);
      void refreshManifest();
    } catch (e) {
      upload = { text: e instanceof Error ? e.message : String(e), error: true };
    }
  }

  const hasFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files');

  function onKey(e: KeyboardEvent) {
    if (e.target instanceof HTMLElement && ['INPUT', 'SELECT', 'TEXTAREA'].includes(e.target.tagName)) return;
    if (e.key === 'Escape') selected = new Set();
    else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      selected = new Set(photos.map((p) => p.hash));
    }
  }

  onMount(() => startPolling(undefined, { idle: 10_000, busy: 3_000 }));
</script>

<svelte:window
  onkeydown={onKey}
  ondragenter={(e) => hasFiles(e) && dragDepth++}
  ondragleave={(e) => hasFiles(e) && (dragDepth = Math.max(0, dragDepth - 1))}
  ondragover={(e) => hasFiles(e) && e.preventDefault()}
  ondrop={(e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    void send([...(e.dataTransfer?.files ?? [])]);
  }}
/>

<div class="manage">
  <header>
    <h1>Photo Frame</h1>
    <nav><a href="/">Frame view</a></nav>
    <span class="spacer"></span>
    {#if upload}<span class="upload" class:error={upload.error} role="status">{upload.text}</span>{/if}
    <button onclick={() => fileInput?.click()}>Upload…</button>
    <input bind:this={fileInput} type="file" multiple accept="image/*,.heic,.heif" hidden
      onchange={(e) => { void send([...(e.currentTarget.files ?? [])]); e.currentTarget.value = ''; }} />
  </header>
  <StatusPanel />
  <div class="body">
    <div class="grid">
      {#if library.error && !library.manifest}
        <p class="empty">Can't reach the server: {library.error}</p>
      {:else}
        <Grid {photos} bind:selected />
      {/if}
    </div>
    <div class="inspector"><Inspector {selection} onclear={() => (selected = new Set())} /></div>
  </div>
  {#if dragDepth > 0}<div class="drop">Drop photographs to upload</div>{/if}
</div>

<Login />

<style>
  .manage { position: fixed; inset: 0; display: grid; grid-template-rows: auto auto 1fr; background: #111; color: #f2f2f7; user-select: text; font-size: 0.95rem; }
  header { display: flex; align-items: center; gap: 16px; padding: 10px 16px; border-bottom: 1px solid #2c2c2e; }
  h1 { margin: 0; font-size: 1.05rem; }
  nav a { color: #0a84ff; text-decoration: none; }
  .spacer { flex: 1; }
  .upload { font-size: 0.85rem; color: #98989f; }
  .upload.error { color: #ff6961; }
  button { font: inherit; padding: 6px 14px; border-radius: 6px; border: 1px solid #3a3a3c; background: #2c2c2e; color: inherit; cursor: pointer; }
  .body { display: grid; grid-template-columns: 1fr 300px; min-height: 0; }
  .grid { min-height: 0; padding: 12px; }
  .inspector { border-left: 1px solid #2c2c2e; min-height: 0; }
  .empty { color: #8e8e93; padding: 24px; }
  .drop { position: absolute; inset: 12px; border: 3px dashed #0a84ff; border-radius: 16px; background: rgb(10 132 255 / 0.12); display: grid; place-items: center; font-size: 1.3rem; pointer-events: none; z-index: 30; }
  @media (max-width: 760px) { .body { grid-template-columns: 1fr; grid-template-rows: 1fr auto; } .inspector { border-left: 0; border-top: 1px solid #2c2c2e; max-height: 45vh; } }
</style>
