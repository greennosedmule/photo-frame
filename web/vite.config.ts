import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    svelte(),
    VitePWA({
      registerType: 'autoUpdate',
      manifest: {
        name: 'Photo Frame',
        short_name: 'Frame',
        display: 'fullscreen',
        background_color: '#000000',
        theme_color: '#000000',
        start_url: '/',
        icons: [
          { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
        ],
      },
      workbox: {
        // Never precache the API or media; those get runtime strategies.
        navigateFallbackDenylist: [/^\/api\//, /^\/media\//],
        runtimeCaching: [
          {
            // Content-addressed and immutable: cache-first, no revalidation.
            urlPattern: ({ url }) => url.pathname.startsWith('/media/'),
            handler: 'CacheFirst',
            options: {
              cacheName: 'media',
              // Least recently used goes first, which is least recently shown; a full
              // disk drops the cache instead of failing the frame.
              expiration: { maxEntries: 6000, purgeOnQuotaError: true },
            },
          },
          {
            // Stale-while-revalidate so a cluster restart never blanks the frame.
            urlPattern: ({ url }) => url.pathname === '/api/manifest',
            handler: 'StaleWhileRevalidate',
            options: { cacheName: 'manifest' },
          },
        ],
      },
    }),
  ],
  // Dev: proxy API and media to a locally running photoframe-web.
  server: {
    proxy: {
      '/api': 'http://localhost:8080',
      '/media': 'http://localhost:8080',
    },
  },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
