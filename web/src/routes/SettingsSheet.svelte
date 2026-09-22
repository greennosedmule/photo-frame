<script lang="ts">
  // Client settings: how *this* frame behaves. Stored in IndexedDB on this
  // device and never sent to the server.
  import { library, settings, updateSettings } from '../lib/state.svelte';
  import Sheet from './Sheet.svelte';

  let { onclose, onchange }: { onclose: () => void; onchange: () => void } = $props();

  const tags = $derived(library.manifest?.tags ?? []);

  function set(change: Parameters<typeof updateSettings>[0]) {
    updateSettings(change);
    onchange();
  }

  function toggleFilter(name: string) {
    set((s) => {
      s.tagFilter = s.tagFilter.includes(name) ? s.tagFilter.filter((t) => t !== name) : [...s.tagFilter, name];
    });
  }
</script>

<Sheet title="Frame settings" {onclose}>
  <p class="muted">These apply to this device only.</p>
  <div class="grid">
    <label>Order
      <select value={settings.ordering} onchange={(e) => set((s) => (s.ordering = e.currentTarget.value as typeof s.ordering))}>
        <option value="shuffle">Shuffle</option>
        <option value="chronological">Oldest first</option>
        <option value="reverse-chronological">Newest first</option>
        <option value="on-this-day">On this day</option>
      </select>
    </label>

    <label>Time per photo: {settings.dwellSeconds}s
      <input type="range" min="5" max="300" step="5" value={settings.dwellSeconds}
        oninput={(e) => set((s) => (s.dwellSeconds = Number(e.currentTarget.value)))} />
    </label>

    <label>Transition
      <select value={settings.transition} onchange={(e) => set((s) => (s.transition = e.currentTarget.value as typeof s.transition))}>
        <option value="crossfade">Crossfade</option>
        <option value="cut">Cut</option>
      </select>
    </label>

    <label>Fill
      <select value={settings.fillMode} onchange={(e) => set((s) => (s.fillMode = e.currentTarget.value as typeof s.fillMode))}>
        <option value="blur">Blurred edges</option>
        <option value="letterbox">Black bars</option>
        <option value="crop">Fill screen</option>
      </select>
    </label>

    <label title="How strongly the next photo tends to share tags with the current one. Off is a plain shuffle; higher stays on a theme longer before wandering off.">Group by tags: {settings.tagAffinity === 0 ? 'off' : settings.tagAffinity.toFixed(2)}
      <input type="range" min="0" max="1" step="0.05" value={settings.tagAffinity}
        oninput={(e) => set((s) => (s.tagAffinity = Number(e.currentTarget.value)))} />
      <small class="muted">Off = shuffle · High = stay on a theme longer</small>
    </label>
  </div>

  <h3>Show only</h3>
  <div class="chips">
    {#each tags as t (t.name)}
      <button class:on={settings.tagFilter.includes(t.name)} onclick={() => toggleFilter(t.name)}>{t.name} <small>{t.count}</small></button>
    {:else}
      <p class="muted">No tags yet.</p>
    {/each}
    {#if settings.tagFilter.length}
      <button class="ghost" onclick={() => set((s) => (s.tagFilter = []))}>Clear</button>
    {/if}
  </div>

  <h3>Dimming</h3>
  <div class="grid">
    <label>From <input type="time" value={settings.dimSchedule.start} onchange={(e) => set((s) => (s.dimSchedule.start = e.currentTarget.value))} /></label>
    <label>Until <input type="time" value={settings.dimSchedule.end} onchange={(e) => set((s) => (s.dimSchedule.end = e.currentTarget.value))} /></label>
    <label>Strength: {Math.round(settings.dimSchedule.opacity * 100)}%
      <input type="range" min="0" max="1" step="0.05" value={settings.dimSchedule.opacity}
        oninput={(e) => set((s) => (s.dimSchedule.opacity = Number(e.currentTarget.value)))} />
    </label>
    <label class="check"><input type="checkbox" checked={settings.dimSchedule.blackout}
        onchange={(e) => set((s) => (s.dimSchedule.blackout = e.currentTarget.checked))} /> Black out completely</label>
  </div>

  <h3>This frame</h3>
  <div class="chips">
    <button disabled={settings.hidden.length === 0} onclick={() => set((s) => (s.hidden = []))}>
      Unhide {settings.hidden.length} hidden photo{settings.hidden.length === 1 ? '' : 's'}
    </button>
    <button disabled={Object.keys(settings.zoom).length === 0} onclick={() => set((s) => (s.zoom = {}))}>
      Forget all remembered zoom
    </button>
    <a class="button" href="/manage">Manage library</a>
  </div>
</Sheet>
