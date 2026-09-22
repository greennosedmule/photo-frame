<script lang="ts">
  // The frame view: full-screen, gesture-driven, and able to run indefinitely
  // from cache with the backend down. Touch and keyboard drive the same actions.
  import { onMount } from 'svelte';
  import { mediaUrl, toggleFavorite, type ManifestPhoto } from '../lib/api';
  import { dimOpacity, TOUCH_LIFT_MS } from '../lib/dim';
  import { attachGestures } from '../lib/gestures';
  import { oledOffset } from '../lib/oled';
  import { prefetchMedia } from '../lib/prefetch';
  import { Sequencer } from '../lib/sequencer';
  import { initSettings, library, patchPhoto, settings, startPolling, updateSettings } from '../lib/state.svelte';
  import { clampRect, rectToTransform, transformToRect, FULL } from '../lib/zoom';
  import Login from './Login.svelte';
  import PhotoSheet from './PhotoSheet.svelte';
  import SettingsSheet from './SettingsSheet.svelte';

  const seq = new Sequencer();
  let surface: HTMLElement | undefined = $state();
  let vw = $state(window.innerWidth);
  let vh = $state(window.innerHeight);

  let currentHash: string | undefined = $state();
  let previousHash: string | undefined = $state();
  let ahead: string[] = $state([]);
  let behind: string | undefined = $state();
  let paused = $state(false);
  let overlayVisible = $state(false);
  /** Which sheet is open. Settings are this device's; the photo sheet acts on the photograph on screen. */
  let sheet: 'settings' | 'photo' | undefined = $state();
  let ready = $state(false);

  const photos = $derived(library.manifest?.photos ?? []);
  const byHash = $derived(new Map(photos.map((p) => [p.hash, p])));
  const current = $derived(currentHash ? byHash.get(currentHash) : undefined);
  /** Slides in the DOM: the one fading out, the one showing, and two preloaded. */
  const slides = $derived(
    [...new Set([previousHash, currentHash, ...ahead, behind].filter((h): h is string => !!h && byHash.has(h)))].map(
      (h) => byHash.get(h)!,
    ),
  );

  // ---- offline cache --------------------------------------------------------------

  let prefetching = false;
  let prefetchAgain = false;
  let destroyed = false;

  /** Keep the whole display set cached; a newer manifest restarts the pass. */
  async function fillCache() {
    if (prefetching) {
      prefetchAgain = true;
      return;
    }
    prefetching = true;
    try {
      do {
        prefetchAgain = false;
        await prefetchMedia(photos, { cancelled: () => destroyed || prefetchAgain });
      } while (prefetchAgain && !destroyed);
    } finally {
      prefetching = false;
    }
  }

  // ---- sequencing ---------------------------------------------------------------

  function plan() {
    ahead = seq.ahead(photos, settings, 2);
    behind = seq.peekPrev(photos, settings);
  }

  function show(hash: string | undefined) {
    if (!hash) return;
    if (hash !== currentHash) previousHash = currentHash;
    currentHash = hash;
    resetZoom(true);
    plan();
    scheduleAdvance();
  }

  function forward() {
    show(seq.next(photos, settings));
  }

  function back() {
    const h = seq.prev(photos, settings);
    if (h) show(h);
  }

  /** Settings or library changed: the preloaded plan may be stale. */
  function replan() {
    seq.dropAhead();
    if (currentHash && !seq.current) return;
    if (current && !isEligible(current)) forward();
    else plan();
  }

  function isEligible(p: ManifestPhoto): boolean {
    return !settings.hidden.includes(p.hash) && (settings.tagFilter.length === 0 || p.tags.some((t) => settings.tagFilter.includes(t)));
  }

  let advanceTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleAdvance() {
    clearTimeout(advanceTimer);
    if (paused) return;
    advanceTimer = setTimeout(() => (dragMode === 'idle' ? forward() : scheduleAdvance()), settings.dwellSeconds * 1000);
  }

  function togglePause() {
    paused = !paused;
    scheduleAdvance();
    showOverlay(); // feedback, and it fades again so nothing sits on the panel
  }

  // ---- drag to swipe ------------------------------------------------------------------

  const SETTLE_MS = 260;
  /** The photograph follows the finger; its neighbour slides in beside it. */
  let dragX = $state(0);
  let dragMode: 'idle' | 'drag' | 'settle' = $state('idle');
  /** Set for one beat after a committed swipe so the new photograph appears without a cross-fade. */
  let snap = $state(false);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;

  function onDrag(dx: number) {
    if (dragMode === 'settle') return;
    dragMode = 'drag';
    dragX = dx;
  }

  function slideOffset(hash: string): number | undefined {
    if (dragMode === 'idle') return undefined;
    if (hash === currentHash) return dragX;
    if (dragX < 0 && hash === ahead[0]) return vw + dragX;
    if (dragX > 0 && hash === behind) return -vw + dragX;
    return undefined;
  }

  function settle(target: number, then?: () => void) {
    dragMode = 'settle';
    dragX = target;
    clearTimeout(settleTimer);
    settleTimer = setTimeout(() => {
      snap = true;
      dragMode = 'idle';
      dragX = 0;
      then?.();
      setTimeout(() => (snap = false), 60);
    }, SETTLE_MS);
  }

  function swipeAway(dir: 'left' | 'right') {
    if (dragMode === 'settle') return;
    if (dragMode === 'idle') {
      // A flick too quick to have moved the photograph: start from rest.
      dragMode = 'drag';
      dragX = 0;
    }
    const has = dir === 'left' ? ahead[0] : behind;
    if (!has) return settle(0);
    settle(dir === 'left' ? -vw : vw, dir === 'left' ? forward : back);
  }

  function onDragEnd() {
    if (dragMode !== 'drag') return;
    if (Math.abs(dragX) > vw * 0.3) swipeAway(dragX < 0 ? 'left' : 'right');
    else settle(0);
  }

  // ---- favourite, hide, share -----------------------------------------------------

  async function favourite() {
    const h = currentHash;
    if (!h) return;
    try {
      const value = await toggleFavorite(h);
      patchPhoto(h, (p) => (p.favorite = value));
    } catch {
      /* backend down: the kiosk keeps running, the change is simply not made */
    }
  }

  function hideCurrent() {
    const h = currentHash;
    if (!h) return;
    updateSettings((s) => {
      if (!s.hidden.includes(h)) s.hidden.push(h);
    });
    seq.dropAhead();
    forward();
  }

  async function share() {
    const h = currentHash;
    if (!h) return;
    try {
      const blob = await (await fetch(mediaUrl(h, 'display'))).blob();
      const file = new File([blob], `photo-${h.slice(0, 8)}.jpg`, { type: 'image/jpeg' });
      if (navigator.canShare?.({ files: [file] })) {
        await navigator.share({ files: [file] });
      } else {
        // Desktop browsers without file sharing get a download.
        const a = document.createElement('a');
        a.href = URL.createObjectURL(file);
        a.download = file.name;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
      }
    } catch {
      /* the user dismissed the share sheet, or the photograph is not cached */
    }
  }

  // ---- zoom -------------------------------------------------------------------------

  let zoom = $state({ scale: 1, tx: 0, ty: 0 });
  let zoomAnimated = $state(false);
  let pinchBase = { scale: 1, tx: 0, ty: 0 };
  const isZoomed = () => zoom.scale > 1.05;

  /** Return to the remembered rectangle, or to the full photograph. */
  function resetZoom(animate: boolean) {
    zoomAnimated = animate;
    const h = currentHash;
    const p = h ? byHash.get(h) : undefined;
    const r = h ? settings.zoom[h] : undefined;
    if (p && r && settings.fillMode !== 'crop') zoom = rectToTransform(clampRect(r), { w: p.w, h: p.h }, { w: vw, h: vh });
    else zoom = { scale: 1, tx: 0, ty: 0 };
  }

  function onPinch(ratio: number, origin: [number, number]) {
    zoomAnimated = false;
    if (ratio === 1 && pinchBase.scale !== zoom.scale) pinchBase = { ...zoom };
    const scale = Math.min(8, Math.max(0.5, pinchBase.scale * ratio));
    const k = scale / pinchBase.scale;
    // Keep the point under the fingers fixed while scaling.
    zoom = { scale, tx: origin[0] - (origin[0] - pinchBase.tx) * k, ty: origin[1] - (origin[1] - pinchBase.ty) * k };
  }

  function onPinchEnd() {
    pinchBase = { ...zoom };
    const h = currentHash;
    const p = h ? byHash.get(h) : undefined;
    if (!h || !p) return;
    // Zooming in is remembered; zooming back out is transient, and the next
    // cycle returns to the remembered rectangle.
    if (zoom.scale > 1.05) {
      const rect = transformToRect(zoom, { w: p.w, h: p.h }, { w: vw, h: vh });
      updateSettings((s) => (s.zoom[h] = rect));
    }
  }

  function onPan(dx: number, dy: number) {
    zoomAnimated = false;
    zoom = { ...zoom, tx: zoom.tx + dx, ty: zoom.ty + dy };
    pinchBase = { ...zoom };
  }

  // ---- dim, wake lock, overlay --------------------------------------------------------

  let liftedUntil = 0;
  let dim = $state(0);
  let offset = $state({ x: 0, y: 0 });
  let overlayTimer: ReturnType<typeof setTimeout> | undefined;

  function tickDim() {
    dim = dimOpacity(new Date(), settings.dimSchedule, liftedUntil);
    offset = oledOffset(Date.now());
  }

  function showOverlay() {
    overlayVisible = true;
    clearTimeout(overlayTimer);
    // Persistent elements must fade when idle, or they burn into the panel.
    overlayTimer = setTimeout(() => (overlayVisible = false), 8000);
  }

  function hideOverlay() {
    overlayVisible = false;
    clearTimeout(overlayTimer);
  }

  function openSheet(which: 'settings' | 'photo') {
    hideOverlay();
    sheet = which;
  }

  /** A right-click is the desktop way to ask for controls, not for the browser's menu. */
  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    showOverlay();
  }

  let wakeLock: WakeLockSentinel | undefined;
  async function acquireWakeLock() {
    try {
      wakeLock = await navigator.wakeLock?.request('screen');
    } catch {
      /* not supported, or the page is not visible yet */
    }
  }

  // ---- input -------------------------------------------------------------------------------

  function onKey(e: KeyboardEvent) {
    if (sheet || (e.target instanceof HTMLElement && ['INPUT', 'SELECT', 'TEXTAREA'].includes(e.target.tagName))) return;
    if (e.key === 'ArrowRight') forward();
    else if (e.key === 'ArrowLeft') back();
    else if (e.key === ' ') {
      e.preventDefault();
      togglePause();
    } else if (e.key === 'f') void favourite();
    else if (e.key === 'i') (overlayVisible ? (overlayVisible = false) : showOverlay());
    else if (e.key === 's') sheet = 'settings';
    else if (e.key === 'Escape') sheet = undefined;
    else return;
    liftedUntil = Date.now() + TOUCH_LIFT_MS;
    tickDim();
  }

  onMount(() => {
    let stopPolling = () => {};
    let detach = () => {};
    const timers: ReturnType<typeof setInterval>[] = [];

    void initSettings().then(() => {
      stopPolling = startPolling(() => {
        if (!ready && photos.length > 0) {
          ready = true;
          forward();
        } else if (ready) {
          replan();
        }
        // After the first photograph is up, so it keeps the network to itself.
        setTimeout(() => void fillCache(), ready ? 3000 : 0);
      });
    });

    if (surface) {
      detach = attachGestures(
        surface,
        {
          touch: () => {
            liftedUntil = Date.now() + TOUCH_LIFT_MS;
            tickDim();
            scheduleAdvance(); // any touch resets the dwell timer
          },
          tap: () => (overlayVisible ? hideOverlay() : showOverlay()),
          doubleTap: () => resetZoom(true),
          longPress: () => (sheet = 'settings'),
          swipe: (dir) => (dir === 'up' ? void share() : swipeAway(dir)),
          drag: onDrag,
          dragEnd: onDragEnd,
          twoFingerSwipeDown: hideCurrent,
          pinch: onPinch,
          pinchEnd: onPinchEnd,
          pan: onPan,
          panEnd: onPinchEnd,
        },
        isZoomed,
      );
    }

    void acquireWakeLock();
    const onVisible = () => document.visibilityState === 'visible' && void acquireWakeLock();
    const onResize = () => {
      vw = window.innerWidth;
      vh = window.innerHeight;
      resetZoom(false);
    };
    document.addEventListener('visibilitychange', onVisible);
    window.addEventListener('resize', onResize);
    tickDim();
    timers.push(setInterval(tickDim, 1000));

    return () => {
      destroyed = true;
      stopPolling();
      detach();
      timers.forEach(clearInterval);
      clearTimeout(advanceTimer);
      clearTimeout(settleTimer);
      clearTimeout(overlayTimer);
      document.removeEventListener('visibilitychange', onVisible);
      window.removeEventListener('resize', onResize);
      void wakeLock?.release();
    };
  });

  const dateFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'long', timeZone: 'UTC' });
  const fmt = (iso: string) => dateFormat.format(new Date(iso));
</script>

<svelte:window onkeydown={onKey} />

<!-- Gestures attach to the photo surface only. The overlay, sheets and dim layer
     are siblings, so a click on a button is never also a tap on the photograph. -->
<div class="frame">
  <main class="surface" bind:this={surface} oncontextmenu={onContextMenu}>
    {#each slides as p (p.hash)}
      {@const off = slideOffset(p.hash)}
      <div class="slide" class:cut={settings.transition === 'cut' || snap || dragMode === 'drag'} class:settling={dragMode === 'settle'}
        style:opacity={p.hash === currentHash || off !== undefined ? 1 : 0}
        style:transform={off !== undefined ? `translateX(${off}px)` : undefined} aria-hidden={p.hash !== currentHash}>
        {#if settings.fillMode === 'blur'}
          <!-- The 64px variant, scaled to fill and blurred in the browser. -->
          <img class="backdrop" src={mediaUrl(p.hash, 'blur', p.media_rotation)} alt="" draggable="false" />
        {/if}
        <div class="zoomer" class:animated={zoomAnimated && p.hash === currentHash}
          style:transform={p.hash === currentHash ? `translate(${zoom.tx}px, ${zoom.ty}px) scale(${zoom.scale})` : undefined}>
          <img class="photo" class:crop={settings.fillMode === 'crop'} src={mediaUrl(p.hash, 'display', p.media_rotation)} alt="" draggable="false" />
        </div>
      </div>
    {/each}

    {#if !ready}
      <p class="message">
        {#if !library.loaded}Loading…
        {:else if library.error && !library.manifest}Waiting for the frame server…
        {:else if photos.length === 0}No photographs yet. <a href="/manage">Add some</a>.
        {/if}
      </p>
    {/if}
  </main>

  {#if current}
    <div class="overlay" class:visible={overlayVisible} style:transform={`translate(${offset.x}px, ${offset.y}px)`}>
      <div class="meta">
        <strong>{fmt(current.effective_date)}</strong>
        {#if current.date_source !== 'exif'}<small>({current.date_source === 'mtime' ? 'file date' : 'edited'})</small>{/if}
        {#if current.tags.length}<span class="tags">{current.tags.join(' · ')}</span>{/if}
      </div>
      <div class="actions">
        <button class="fav" class:on={current.favorite} onclick={favourite} aria-label={current.favorite ? 'Unfavourite' : 'Favourite'}>
          {current.favorite ? '♥' : '♡'}
        </button>
        <button onclick={() => openSheet('photo')} aria-label="This photograph">Photo</button>
        <button onclick={() => openSheet('settings')} aria-label="Frame settings">⚙</button>
      </div>
    </div>
  {/if}

  {#if paused && overlayVisible}<div class="paused">Paused</div>{/if}
  <div class="dim" style:opacity={dim}></div>
</div>

{#if sheet === 'settings'}
  <SettingsSheet onclose={() => (sheet = undefined)} onchange={replan} />
{:else if sheet === 'photo' && currentHash}
  <PhotoSheet hash={currentHash} onclose={() => (sheet = undefined)} onhide={() => { sheet = undefined; hideCurrent(); }} onshare={() => void share()} />
{/if}

<Login />

<style>
  .frame { position: fixed; inset: 0; overflow: hidden; background: #000; }
  .surface {
    position: absolute; inset: 0; overflow: hidden; background: #000;
    touch-action: none; -webkit-touch-callout: none; -webkit-user-select: none; user-select: none;
  }
  .slide { position: absolute; inset: 0; transition: opacity 1.2s ease; }
  .slide.cut { transition: none; }
  .slide.settling { transition: transform 0.26s ease-out; }
  .backdrop { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; filter: blur(28px) brightness(0.45); transform: scale(1.15); }
  .zoomer { position: absolute; inset: 0; transform-origin: 0 0; will-change: transform; }
  .zoomer.animated { transition: transform 0.35s ease; }
  .photo { width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .photo.crop { object-fit: cover; }
  .message { position: absolute; inset: 0; display: grid; place-items: center; margin: 0; color: #8e8e93; }
  .message a { color: #0a84ff; }
  .overlay {
    position: absolute; left: 0; right: 0; bottom: 0; display: flex; justify-content: space-between; align-items: flex-end; gap: 16px;
    padding: 24px calc(28px + env(safe-area-inset-right)) calc(24px + env(safe-area-inset-bottom)) calc(28px + env(safe-area-inset-left));
    background: linear-gradient(transparent, rgb(0 0 0 / 0.65)); opacity: 0; pointer-events: none; transition: opacity 0.8s ease;
  }
  .overlay.visible { opacity: 1; }
  /* The bar itself never intercepts the photograph's gestures; only its buttons do. */
  .overlay.visible .actions { pointer-events: auto; }
  .actions { display: flex; align-items: center; gap: 10px; pointer-events: none; }
  .actions button { font: inherit; font-size: 1rem; line-height: 1; padding: 10px 16px; border-radius: 999px; border: 1px solid rgb(255 255 255 / 0.25); background: rgb(0 0 0 / 0.45); color: #fff; cursor: pointer; }
  .meta { display: grid; gap: 4px; font-size: 1.1rem; text-shadow: 0 1px 4px #000; }
  .meta small { opacity: 0.7; font-size: 0.85rem; }
  .tags { opacity: 0.85; font-size: 0.95rem; }
  .actions .fav { font-size: 1.4rem; padding: 8px 14px; }
  .actions .fav.on { color: #ff453a; }
  .paused { position: absolute; top: calc(20px + env(safe-area-inset-top)); right: 24px; padding: 4px 12px; border-radius: 999px; background: rgb(0 0 0 / 0.55); font-size: 0.85rem; }
  .dim { position: absolute; inset: 0; background: #000; pointer-events: none; transition: opacity 1s linear; }
</style>
