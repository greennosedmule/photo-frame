<script lang="ts">
  // Virtualised thumbnail grid: only the rows in view are in the DOM. Selection
  // supports click, ctrl/cmd-click, shift-click ranges and a rubber-band drag.
  import type { ManifestPhoto } from '../lib/api';
  import { mediaUrl } from '../lib/api';
  import { CELL, PITCH, cellRect, columnsFor, indexRange, indicesInRect, normalizeRect, rowCount, visibleRows, type Rect } from '../lib/grid';

  let { photos, selected = $bindable() }: { photos: ManifestPhoto[]; selected: Set<string> } = $props();

  let scroller: HTMLDivElement | undefined = $state();
  let width = $state(800);
  let height = $state(600);
  let scrollTop = $state(0);
  let anchor = -1;

  const cols = $derived(columnsFor(width));
  const rows = $derived(rowCount(photos.length, cols));
  const range = $derived(visibleRows(scrollTop, height, rows));
  const first = $derived(range[0] * cols);
  const last = $derived(Math.min(photos.length, (range[1] + 1) * cols));
  const visible = $derived(photos.slice(first, last).map((p, i) => ({ p, i: first + i })));

  const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeZone: 'UTC' });
  const caption = (p: ManifestPhoto) => {
    const date = dateFormat.format(new Date(p.effective_date));
    return p.tags.length ? `${date} · ${p.tags.length} tag${p.tags.length > 1 ? 's' : ''}` : date;
  };
  const sourceLabel = { exif: 'EXIF', mtime: 'file date', override: 'edited' } as const;

  // ---- click selection ------------------------------------------------------------
  let suppressClick = false;

  function click(e: MouseEvent, index: number) {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    const hash = photos[index]!.hash;
    if (e.shiftKey && anchor >= 0) {
      const base = e.ctrlKey || e.metaKey ? new Set(selected) : new Set<string>();
      for (const i of indexRange(anchor, index)) base.add(photos[i]!.hash);
      selected = base;
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      if (next.has(hash)) next.delete(hash);
      else next.add(hash);
      selected = next;
      anchor = index;
    } else {
      selected = new Set([hash]);
      anchor = index;
    }
  }

  // ---- rubber band -----------------------------------------------------------------
  let band: Rect | undefined = $state();
  let bandStart: { x: number; y: number; base: Set<string>; active: boolean } | undefined;

  /** Pointer position in content coordinates (scroll included). */
  function contentPoint(e: PointerEvent): [number, number] {
    const r = scroller!.getBoundingClientRect();
    return [e.clientX - r.left + scroller!.scrollLeft, e.clientY - r.top + scroller!.scrollTop];
  }

  function down(e: PointerEvent) {
    if (e.button !== 0 || !scroller) return;
    // Leave the scrollbar alone.
    if (e.offsetX > scroller.clientWidth && e.target === scroller) return;
    const [x, y] = contentPoint(e);
    bandStart = { x, y, base: e.ctrlKey || e.metaKey || e.shiftKey ? new Set(selected) : new Set(), active: false };
  }

  function move(e: PointerEvent) {
    if (!bandStart || !scroller) return;
    const [x, y] = contentPoint(e);
    if (!bandStart.active) {
      if (Math.hypot(x - bandStart.x, y - bandStart.y) < 6) return;
      bandStart.active = true;
      scroller.setPointerCapture(e.pointerId);
    }
    band = normalizeRect(bandStart.x, bandStart.y, x, y);
    const next = new Set(bandStart.base);
    for (const i of indicesInRect(band, cols, photos.length)) next.add(photos[i]!.hash);
    selected = next;
  }

  function up() {
    if (bandStart?.active) suppressClick = true; // the drag must not also count as a click
    bandStart = undefined;
    band = undefined;
    // A click never follows a drag that ended elsewhere; clear the flag next tick.
    setTimeout(() => (suppressClick = false), 0);
  }

  // Keep the anchor sensible when the list changes underneath us.
  $effect(() => {
    if (anchor >= photos.length) anchor = -1;
  });
</script>

<div class="scroller" bind:this={scroller} bind:clientWidth={width} bind:clientHeight={height}
  onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} role="listbox" aria-multiselectable="true" tabindex="0">
  <div class="spacer" style:height={`${Math.max(0, rows * PITCH - (rows ? PITCH - CELL : 0))}px`}>
    {#each visible as { p, i } (p.hash)}
      {@const r = cellRect(i, cols)}
      <button class="cell" class:selected={selected.has(p.hash)} role="option" aria-selected={selected.has(p.hash)}
        style:left={`${r.x}px`} style:top={`${r.y}px`} style:width={`${CELL}px`} style:height={`${CELL}px`}
        onclick={(e) => click(e, i)}>
        <img src={mediaUrl(p.hash, 'thumb', p.media_rotation)} alt="" loading="lazy" draggable="false" />
        <span class="badge {p.date_source}" title={`Date from ${sourceLabel[p.date_source]}`}>{sourceLabel[p.date_source]}</span>
        {#if p.favorite}<span class="fav">♥</span>{/if}
        {#if p.rotation !== p.media_rotation}<span class="rotating" title="Regenerating after a rotation">rotating…</span>{/if}
        <span class="date">{caption(p)}</span>
      </button>
    {/each}
    {#if band}<div class="band" style:left={`${band.x}px`} style:top={`${band.y}px`} style:width={`${band.w}px`} style:height={`${band.h}px`}></div>{/if}
  </div>
  {#if photos.length === 0}
    <p class="empty">No photographs yet. Drop files here to upload.</p>
  {/if}
</div>

<style>
  .scroller { position: relative; overflow-y: auto; height: 100%; user-select: none; outline: none; scrollbar-color: #48484a #111; }
  .spacer { position: relative; }
  .cell { position: absolute; padding: 0; border: 0; background: #2c2c2e; overflow: hidden; border-radius: 6px; color: #fff; cursor: pointer; }
  .cell img { width: 100%; height: 100%; object-fit: cover; display: block; pointer-events: none; }
  .cell.selected { outline: 3px solid #0a84ff; outline-offset: -3px; }
  .cell.selected::after { content: ''; position: absolute; inset: 0; background: rgb(10 132 255 / 0.25); }
  .badge { position: absolute; top: 6px; left: 6px; font-size: 0.65rem; padding: 2px 6px; border-radius: 999px; background: rgb(0 0 0 / 0.6); }
  /* Photographs whose date is a guess stand out. */
  .badge.mtime { background: #b25000; }
  .badge.override { background: #30619e; }
  .rotating { position: absolute; top: 30px; left: 6px; font-size: 0.65rem; padding: 2px 6px; border-radius: 999px; background: #0a84ff; }
  .fav { position: absolute; top: 4px; right: 8px; color: #ff453a; text-shadow: 0 1px 3px #000; }
  .date { position: absolute; left: 0; right: 0; bottom: 0; padding: 14px 8px 5px; font-size: 0.7rem; text-align: left; background: linear-gradient(transparent, rgb(0 0 0 / 0.7)); }
  .band { position: absolute; border: 1px solid #0a84ff; background: rgb(10 132 255 / 0.2); pointer-events: none; z-index: 2; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; margin: 0; color: #8e8e93; pointer-events: none; }
</style>
