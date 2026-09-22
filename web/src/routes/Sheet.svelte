<script lang="ts">
  // A bottom sheet over the frame. Lives outside the gesture surface, so taps
  // and drags inside it never reach the photograph.
  import type { Snippet } from 'svelte';
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  let { title, onclose, children }: { title: string; onclose: () => void; children: Snippet } = $props();
</script>

<div class="scrim" role="presentation" onclick={onclose} transition:fade|global={{ duration: 250 }}></div>
<section class="sheet" aria-label={title} transition:fly|global={{ y: window.innerHeight, duration: 350, easing: cubicOut, opacity: 1 }}>
  <header>
    <h2>{title}</h2>
    <button onclick={onclose}>Done</button>
  </header>
  {@render children()}
</section>

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.5); z-index: 20; }
  .sheet {
    position: fixed; z-index: 21; left: 0; right: 0; bottom: 0; margin: 0 auto;
    width: min(660px, 100%); max-height: 85vh; overflow: auto; box-sizing: border-box;
    background: #1c1c1e; color: #f2f2f7; border-radius: 16px 16px 0 0;
    padding: 12px 20px calc(20px + env(safe-area-inset-bottom));
    -webkit-user-select: none; user-select: none; color-scheme: dark;
  }
  header { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font-size: 1.1rem; }
  /* Shared control styling for everything inside a sheet. */
  .sheet :global(h3) { margin: 1.1rem 0 0.4rem; font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.06em; color: #98989f; }
  .sheet :global(.grid) { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 12px 20px; margin-top: 12px; }
  .sheet :global(label) { display: grid; gap: 4px; font-size: 0.9rem; }
  .sheet :global(label.check) { display: flex; align-items: center; gap: 8px; }
  .sheet :global(select), .sheet :global(input:not([type='checkbox']):not([type='range'])) { font: inherit; padding: 6px 8px; border-radius: 8px; border: 1px solid #3a3a3c; background: #2c2c2e; color: inherit; }
  .sheet :global(input[type='range']) { width: 100%; }
  .sheet :global(.chips) { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .sheet :global(.chips form) { display: inline-flex; gap: 6px; }
  .sheet :global(.chips form input) { width: 8rem; }
  .sheet :global(button), .sheet :global(a.button) { font: inherit; padding: 7px 14px; border-radius: 999px; border: 1px solid #3a3a3c; background: #2c2c2e; color: inherit; text-decoration: none; cursor: pointer; }
  .sheet :global(button.on) { background: #0a84ff; border-color: #0a84ff; }
  .sheet :global(button.ghost) { background: none; }
  .sheet :global(button:disabled) { opacity: 0.4; cursor: default; }
  .sheet :global(small) { opacity: 0.6; }
  .sheet :global(.muted) { color: #98989f; margin: 0; }
  .sheet :global(.error) { color: #ff6961; }
  .sheet :global(.busy) { color: #0a84ff; }
</style>
