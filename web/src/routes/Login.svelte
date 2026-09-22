<script lang="ts">
  import { authPrompt, cancelLogin, submitLogin } from '../lib/auth.svelte';

  let user = $state('admin');
  let password = $state('');
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    await submitLogin(user, password);
    busy = false;
    password = '';
  }
</script>

{#if authPrompt.open}
  <div class="scrim" role="presentation">
    <form class="card" onsubmit={submit}>
      <h2>Sign in to manage</h2>
      <label>Username <input bind:value={user} autocomplete="username" required /></label>
      <label>Password <input type="password" bind:value={password} autocomplete="current-password" required /></label>
      {#if authPrompt.error}<p class="error" role="alert">{authPrompt.error}</p>{/if}
      <div class="row">
        <button type="button" onclick={cancelLogin}>Cancel</button>
        <button type="submit" class="primary" disabled={busy}>{busy ? 'Checking…' : 'Sign in'}</button>
      </div>
    </form>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.6); display: grid; place-items: center; z-index: 50; }
  .card { background: #1c1c1e; padding: 24px; border-radius: 12px; display: grid; gap: 12px; width: min(360px, 90vw); }
  h2 { margin: 0 0 4px; font-size: 1.1rem; }
  label { display: grid; gap: 4px; font-size: 0.85rem; color: #98989f; }
  input { font: inherit; padding: 8px 10px; border-radius: 8px; border: 1px solid #3a3a3c; background: #2c2c2e; color: #f2f2f7; }
  .row { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  button { font: inherit; padding: 8px 16px; border-radius: 8px; border: 1px solid #3a3a3c; background: #2c2c2e; color: inherit; }
  button.primary { background: #0a84ff; border-color: #0a84ff; }
  .error { color: #ff6961; margin: 0; font-size: 0.9rem; }
</style>
