import { mount } from 'svelte';
import { registerSW } from 'virtual:pwa-register';
import './app.css';
import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });

// A frame never navigates, so it must fetch new builds itself. `autoUpdate`
// reloads the page once a new service worker has taken over; checking hourly
// means a redeploy reaches a running frame without anyone touching it.
registerSW({
  immediate: true,
  onRegisteredSW(_url, registration) {
    if (registration) setInterval(() => void registration.update().catch(() => {}), 60 * 60 * 1000);
  },
});
