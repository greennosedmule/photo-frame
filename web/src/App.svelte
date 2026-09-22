<script lang="ts">
  import Frame from './routes/Frame.svelte';
  import Manage from './routes/Manage.svelte';

  // Two views, one build. Deliberately not one responsive layout.
  const isManage = location.pathname.startsWith('/manage');
</script>

{#if isManage}
  <Manage />
{:else}
  <Frame />
{/if}
