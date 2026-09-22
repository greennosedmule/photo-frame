<script lang="ts">
  // Actions on the selection: tags, date override (with its source shown), delete.
  import { addTag, createTag, deletePhoto, removeTag, rotatePhoto, setDateOverride, type ManifestPhoto } from '../lib/api';
  import { withAuth } from '../lib/auth.svelte';
  import { pool } from '../lib/pool';
  import { library, refreshManifest, waitForRotations } from '../lib/state.svelte';

  let { selection, onclear }: { selection: ManifestPhoto[]; onclear: () => void } = $props();

  const tags = $derived(library.manifest?.tags ?? []);
  let busy = $state('');
  let message = $state('');
  let newTag = $state('');
  let dateInput = $state('');
  let confirmDelete = $state(false);

  const CONCURRENCY = 6;

  function tagState(name: string): 'all' | 'some' | 'none' {
    const n = selection.filter((p) => p.tags.includes(name)).length;
    return n === 0 ? 'none' : n === selection.length ? 'all' : 'some';
  }

  const sources = $derived.by(() => {
    const c = { exif: 0, mtime: 0, override: 0 };
    for (const p of selection) c[p.date_source]++;
    return c;
  });

  const dateRange = $derived.by(() => {
    if (selection.length === 0) return '';
    const ds = selection.map((p) => p.effective_date).sort();
    const f = (iso: string) => new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeZone: 'UTC' }).format(new Date(iso));
    return ds[0] === ds.at(-1) ? f(ds[0]!) : `${f(ds[0]!)} – ${f(ds.at(-1)!)}`;
  });

  async function run(label: string, items: ManifestPhoto[], fn: (p: ManifestPhoto) => Promise<void>) {
    busy = `${label}…`;
    message = '';
    try {
      const { failed, error } = await pool(items, CONCURRENCY, fn, (d) => (busy = `${label} ${d}/${items.length}…`));
      if (failed.length) message = `${failed.length} failed: ${error instanceof Error ? error.message : error}`;
    } finally {
      busy = '';
      await refreshManifest();
    }
  }

  async function toggleTag(name: string) {
    // Everything has it: remove from all. Otherwise add to those missing it.
    if (tagState(name) === 'all') await run('Removing tag', selection, (p) => removeTag(p.hash, name));
    else await run('Tagging', selection.filter((p) => !p.tags.includes(name)), (p) => addTag(p.hash, name));
  }

  async function createAndApply() {
    const name = newTag.trim();
    if (!name) return;
    try {
      const canonical = await createTag(name);
      newTag = '';
      await run('Tagging', selection, (p) => addTag(p.hash, canonical));
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
  }

  /** `datetime-local` gives a wall-clock time; like EXIF dates, keep it as written. */
  const toIso = (local: string) => (local.length === 16 ? `${local}:00Z` : `${local}Z`);

  async function applyDate() {
    if (!dateInput) return;
    const iso = toIso(dateInput);
    try {
      await withAuth(() => pool(selection, CONCURRENCY, (p) => setDateOverride(p.hash, iso)).then(check));
      dateInput = '';
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
    await refreshManifest();
  }

  async function clearDate() {
    const overridden = selection.filter((p) => p.date_source === 'override');
    try {
      await withAuth(() => pool(overridden, CONCURRENCY, (p) => setDateOverride(p.hash, null)).then(check));
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
    await refreshManifest();
  }

  /** Surface a partial failure; an auth failure must propagate so login can retry. */
  function check(r: { failed: unknown[]; error?: unknown }) {
    if (r.failed.length) throw r.error instanceof Error ? r.error : new Error('some updates failed');
  }

  /** Quarter turns for the whole selection. Relative, so each photograph turns
   *  from wherever it is now. The originals are untouched; the indexer makes
   *  new derivatives and the grid shows "rotating…" until they exist. */
  async function rotate(delta: number) {
    const items = [...selection];
    message = '';
    busy = 'Rotating…';
    try {
      await withAuth(() => pool(items, CONCURRENCY, (p) => rotatePhoto(p.hash, delta)).then(check));
      await refreshManifest();
      if (!(await waitForRotations(items.map((p) => p.hash)))) message = 'The server is taking a while to rotate these.';
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    } finally {
      busy = '';
      await refreshManifest();
    }
  }

  async function doDelete() {
    confirmDelete = false;
    const items = [...selection];
    busy = 'Deleting…';
    try {
      await withAuth(() => pool(items, CONCURRENCY, (p) => deletePhoto(p.hash)).then(check));
      onclear();
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    } finally {
      busy = '';
      await refreshManifest();
    }
  }
</script>

<aside>
  {#if selection.length === 0}
    <p class="muted">Select photographs to tag, re-date or delete them. Shift-click for a range, or drag a box.</p>
  {:else}
    <h2>{selection.length} selected</h2>
    <p class="muted">{dateRange}</p>

    <h3>Tags</h3>
    <ul class="tags">
      {#each tags as t (t.name)}
        {@const st = tagState(t.name)}
        <li>
          <label>
            <input type="checkbox" checked={st === 'all'} indeterminate={st === 'some'} disabled={!!busy}
              onchange={() => toggleTag(t.name)} />
            {t.name} <small>{t.count}</small>
          </label>
        </li>
      {/each}
    </ul>
    <form class="inline" onsubmit={(e) => { e.preventDefault(); void createAndApply(); }}>
      <input placeholder="New tag" bind:value={newTag} maxlength="64" />
      <button type="submit" disabled={!newTag.trim() || !!busy} aria-label="Create tag and apply">+</button>
    </form>

    <h3>Rotate</h3>
    <div class="inline">
      <button onclick={() => rotate(-90)} disabled={!!busy} aria-label="Rotate left">⟲ Left</button>
      <button onclick={() => rotate(90)} disabled={!!busy} aria-label="Rotate right">⟳ Right</button>
      <button onclick={() => rotate(180)} disabled={!!busy} aria-label="Rotate 180 degrees">180°</button>
    </div>
    <p class="hint">Changes what every frame shows. Originals are never modified.</p>

    <h3>Date</h3>
    <p class="muted">
      {#if sources.exif}{sources.exif} from EXIF{/if}{#if sources.mtime}{sources.exif ? ', ' : ''}<b class="warn">{sources.mtime} guessed from file date</b>{/if}{#if sources.override}{sources.exif || sources.mtime ? ', ' : ''}{sources.override} edited{/if}
    </p>
    <div class="inline">
      <input type="datetime-local" bind:value={dateInput} />
      <button onclick={applyDate} disabled={!dateInput || !!busy}>Set</button>
    </div>
    {#if sources.override}
      <button class="link" onclick={clearDate} disabled={!!busy}>Clear {sources.override} override{sources.override > 1 ? 's' : ''}</button>
    {/if}
    <p class="hint">Only the database changes; original files are never modified.</p>

    <h3>Delete</h3>
    <button class="danger" onclick={() => (confirmDelete = true)} disabled={!!busy}>Delete {selection.length} photograph{selection.length > 1 ? 's' : ''}…</button>
  {/if}

  {#if busy}<p class="busy" role="status">{busy}</p>{/if}
  {#if message}<p class="error" role="alert">{message}</p>{/if}
</aside>

{#if confirmDelete}
  <div class="scrim" role="presentation">
    <div class="dialog" role="alertdialog" aria-labelledby="del-title">
      <h2 id="del-title">Delete {selection.length} photograph{selection.length > 1 ? 's' : ''}?</h2>
      <p>This removes the original file{selection.length > 1 ? 's' : ''} from disk. It can't be undone.</p>
      <div class="row">
        <button onclick={() => (confirmDelete = false)}>Cancel</button>
        <button class="danger" onclick={doDelete}>Delete {selection.length} file{selection.length > 1 ? 's' : ''}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* minmax(0, 1fr): a wide control (the date input) must shrink, not widen the column. */
  aside { padding: 16px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; align-content: start; overflow-y: auto; overflow-x: hidden; height: 100%; box-sizing: border-box; }
  h2 { margin: 0; font-size: 1.05rem; }
  h3 { margin: 14px 0 2px; font-size: 0.72rem; text-transform: uppercase; letter-spacing: 0.06em; color: #98989f; }
  p { margin: 0; }
  .muted { color: #98989f; font-size: 0.85rem; }
  .warn { color: #ff9f0a; font-weight: 600; }
  .hint { color: #6c6c70; font-size: 0.75rem; }
  .tags { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; max-height: 240px; overflow-y: auto; }
  .tags label { display: flex; gap: 8px; align-items: center; padding: 3px 0; }
  small { color: #6c6c70; }
  .inline { display: flex; gap: 6px; }
  .inline input { flex: 1; min-width: 0; width: 0; }
  input:not([type='checkbox']) { font: inherit; padding: 6px 8px; border-radius: 6px; border: 1px solid #3a3a3c; background: #2c2c2e; color: #f2f2f7; color-scheme: dark; }
  button { font: inherit; padding: 6px 14px; border-radius: 6px; border: 1px solid #3a3a3c; background: #2c2c2e; color: inherit; cursor: pointer; }
  button:disabled { opacity: 0.4; cursor: default; }
  button.link { background: none; border: 0; color: #0a84ff; padding: 2px 0; text-align: left; }
  button.danger { background: #ff453a; border-color: #ff453a; color: #fff; }
  .busy { color: #0a84ff; }
  .error { color: #ff6961; font-size: 0.85rem; }
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.6); display: grid; place-items: center; z-index: 40; }
  .dialog { background: #1c1c1e; padding: 22px; border-radius: 12px; width: min(400px, 90vw); display: grid; gap: 10px; }
  .row { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }
</style>
