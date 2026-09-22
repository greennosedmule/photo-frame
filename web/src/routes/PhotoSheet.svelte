<script lang="ts">
  // Actions on the photograph on screen. Favourites, tags and rotation change
  // the shared library (every frame sees them); hiding and zoom are this
  // device's own.
  import { addTag, createTag, removeTag, rotatePhoto, toggleFavorite } from '../lib/api';
  import { withAuth } from '../lib/auth.svelte';
  import { library, patchPhoto, refreshManifest, settings, updateSettings, waitForRotations } from '../lib/state.svelte';
  import Sheet from './Sheet.svelte';

  let { hash, onclose, onhide, onshare }: {
    hash: string;
    onclose: () => void;
    onhide: () => void;
    onshare: () => void;
  } = $props();

  const photo = $derived(library.manifest?.photos.find((p) => p.hash === hash));
  const tags = $derived(library.manifest?.tags ?? []);
  let newTag = $state('');
  let message = $state('');
  let busy = $state('');

  const sourceText = $derived(
    !photo ? '' : photo.date_source === 'exif' ? 'date from the photo' : photo.date_source === 'mtime' ? 'date guessed from the file' : 'date edited',
  );
  const rotating = $derived(!!photo && photo.rotation !== photo.media_rotation);

  async function favourite() {
    try {
      const value = await toggleFavorite(hash);
      patchPhoto(hash, (p) => (p.favorite = value));
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
  }

  async function toggleOnPhoto(name: string) {
    if (!photo) return;
    const has = photo.tags.includes(name);
    try {
      if (has) await removeTag(hash, name);
      else await addTag(hash, name);
      patchPhoto(hash, (p) => {
        p.tags = has ? p.tags.filter((t) => t !== name) : [...p.tags, name].sort();
      });
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
  }

  async function create() {
    const name = newTag.trim();
    if (!name) return;
    try {
      const canonical = await createTag(name);
      newTag = '';
      if (photo && !photo.tags.includes(canonical)) await toggleOnPhoto(canonical);
      await refreshManifest();
      message = '';
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    }
  }

  async function rotate(delta: number) {
    message = '';
    try {
      await withAuth(() => rotatePhoto(hash, delta));
      // A remembered zoom rectangle belongs to the old orientation.
      updateSettings((s) => delete s.zoom[hash]);
      busy = 'Rotating…';
      await refreshManifest();
      if (!(await waitForRotations([hash]))) message = 'The server is taking a while to rotate this photograph.';
    } catch (e) {
      message = e instanceof Error ? e.message : String(e);
    } finally {
      busy = '';
    }
  }
</script>

<Sheet title="This photograph" {onclose}>
  {#if photo}
    <p class="muted">{new Intl.DateTimeFormat(undefined, { dateStyle: 'long', timeZone: 'UTC' }).format(new Date(photo.effective_date))} · {sourceText}</p>

    <h3>For everyone</h3>
    <div class="chips">
      <button class:on={photo.favorite} onclick={favourite}>{photo.favorite ? '♥ Favourite' : '♡ Favourite'}</button>
      <button onclick={() => rotate(-90)} disabled={!!busy || rotating} aria-label="Rotate left">⟲ Rotate left</button>
      <button onclick={() => rotate(90)} disabled={!!busy || rotating} aria-label="Rotate right">⟳ Rotate right</button>
      {#if busy || rotating}<span class="busy" role="status">Rotating…</span>{/if}
    </div>
    <p class="muted rot">Rotation changes what every frame shows. The original file is never modified.</p>

    <h3>Tags</h3>
    <div class="chips">
      {#each tags as t (t.name)}
        <button class:on={photo.tags.includes(t.name)} onclick={() => toggleOnPhoto(t.name)}>{t.name}</button>
      {/each}
      <form onsubmit={(e) => { e.preventDefault(); void create(); }}>
        <input placeholder="New tag" bind:value={newTag} maxlength="64" />
        <button type="submit" aria-label="Create tag">+</button>
      </form>
    </div>

    <h3>On this frame only</h3>
    <div class="chips">
      <button onclick={onhide}>Hide this photo</button>
      <button disabled={!(hash in settings.zoom)} onclick={() => updateSettings((s) => delete s.zoom[hash])}>Forget remembered zoom</button>
      <button onclick={onshare}>Share…</button>
    </div>
    {#if message}<p class="error" role="alert">{message}</p>{/if}
  {:else}
    <p class="muted">This photograph is no longer in the library.</p>
  {/if}
</Sheet>

<style>
  .rot { font-size: 0.8rem; margin-top: 6px; }
</style>
