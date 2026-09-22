<script lang="ts">
  import { onMount } from 'svelte';
  import { downloadExport, getRequest, getStatus, requestExport, requestScan, type Status } from '../lib/api';
  import { withAuth } from '../lib/auth.svelte';

  let status: Status | undefined = $state();
  let error = $state('');
  let note = $state('');
  let busy = $state(false);

  async function poll() {
    try {
      status = await getStatus();
      error = '';
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    void poll();
    const t = setInterval(poll, 3000);
    return () => clearInterval(t);
  });

  async function scan() {
    try {
      await withAuth(requestScan);
      note = 'Scan requested.';
      void poll();
    } catch (e) {
      note = e instanceof Error ? e.message : String(e);
    }
  }

  /** Ask the indexer for an export, wait for it, then hand the file to the browser. */
  async function exportCuration() {
    busy = true;
    note = 'Exporting…';
    try {
      const id = await withAuth(requestExport);
      for (let i = 0; i < 60; i++) {
        const r = await withAuth(() => getRequest(id));
        if (r.state === 'done' && r.result) {
          const blob = await withAuth(() => downloadExport(r.result!));
          const a = document.createElement('a');
          a.href = URL.createObjectURL(blob);
          a.download = r.result;
          a.click();
          setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
          note = `Saved ${r.result}.`;
          return;
        }
        if (r.state === 'failed') throw new Error('The indexer could not write the export.');
        await new Promise((res) => setTimeout(res, 1000));
      }
      throw new Error('Timed out waiting for the indexer.');
    } catch (e) {
      note = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  const when = (iso: string | null) => (iso ? new Date(iso).toLocaleString() : 'never');
  const phase = $derived(
    !status ? '' : !status.indexing ? 'Idle' : status.derivative_queue > 0 && status.scan_total > 0 ? `Working: ${status.scan_done} of ${status.scan_total}` : 'Scanning',
  );
</script>

<section class="panel" aria-label="Status">
  {#if status}
    <span><b>{status.photo_count}</b> photographs</span>
    <span class:active={status.indexing}>{phase}</span>
    <span title="Last completed scan">Scanned {when(status.last_scan_at)}</span>
    <span>Queue <b>{status.derivative_queue}</b></span>
    <span class:bad={status.derivative_failures > 0}>Failed <b>{status.derivative_failures}</b></span>
  {:else}
    <span class:bad={!!error}>{error || 'Loading…'}</span>
  {/if}
  <span class="spacer"></span>
  {#if note}<span class="note">{note}</span>{/if}
  <button onclick={scan}>Scan now</button>
  <button onclick={exportCuration} disabled={busy}>Export curation</button>
</section>

<style>
  .panel { display: flex; gap: 8px 20px; align-items: center; flex-wrap: wrap; padding: 10px 16px; font-size: 0.85rem; color: #98989f; border-bottom: 1px solid #2c2c2e; }
  b { color: #f2f2f7; }
  .active { color: #0a84ff; }
  .bad, .bad b { color: #ff6961; }
  .spacer { flex: 1; }
  .note { color: #f2f2f7; }
  button { font: inherit; padding: 5px 12px; border-radius: 6px; border: 1px solid #3a3a3c; background: #2c2c2e; color: #f2f2f7; cursor: pointer; }
  button:disabled { opacity: 0.4; }
</style>
