import { type Page, test as base, expect } from '@playwright/test';
import { ADMIN_PASSWORD, Stack } from './stack';

export { expect };

/**
 * Run `fn` and, on failure, fold the tail of the stack's server output into
 * the thrown error so it shows up in the CI annotation, not just the trace
 * artifact (which needs an authenticated download to read).
 */
export async function withServerLog<T>(stack: Stack, fn: () => Promise<T>): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    const tail = stack.logs().slice(-8000);
    throw new Error(`${e instanceof Error ? e.message : e}\n--- server log (tail) ---\n${tail}`);
  }
}

/**
 * `stack` is one real server plus indexer per worker, on its own volume and
 * port. `photoCount` sets how many photographs it starts with.
 */
export const test = base.extend<object, { stack: Stack; photoCount: number }>({
  photoCount: [12, { scope: 'worker', option: true }],
  stack: [
    async ({ photoCount }, use) => {
      const stack = await Stack.start(photoCount);
      await use(stack);
      await stack.stop();
    },
    { scope: 'worker' },
  ],
  baseURL: async ({ stack }, use) => use(stack.url),
});

/** Answer the management login form. */
export async function signIn(page: Page, password = ADMIN_PASSWORD): Promise<void> {
  await page.locator('.card input[autocomplete="current-password"]').fill(password);
  await page.locator('.card button.primary').click();
}

/** Opacity really settled, not just the class: the overlay fades over 0.8 s. */
export async function overlayShown(page: Page): Promise<boolean> {
  await page.waitForTimeout(1000);
  return page.evaluate(() => {
    const o = document.querySelector('.overlay');
    return !!o && o.classList.contains('visible') && Number(getComputedStyle(o).opacity) > 0.95;
  });
}

/** `src` of the photograph currently showing on the frame. */
export function currentSrc(page: Page): Promise<string> {
  return page.evaluate(() => {
    const slide = [...document.querySelectorAll<HTMLElement>('.slide')].find((s) => s.style.opacity === '1');
    return slide?.querySelector('img.photo')?.getAttribute('src') ?? '';
  });
}

export const hashOf = (src: string): string => src.split('/')[2] ?? '';

export function settingsInIndexedDb(page: Page): Promise<Record<string, unknown> | undefined> {
  return page.evaluate(
    () =>
      new Promise((resolve) => {
        const open = indexedDB.open('photoframe');
        open.onerror = () => resolve(undefined);
        open.onsuccess = () => {
          if (!open.result.objectStoreNames.contains('kv')) return resolve(undefined);
          const get = open.result.transaction('kv').objectStore('kv').get('settings');
          get.onsuccess = () => resolve(get.result);
          get.onerror = () => resolve(undefined);
        };
      }),
  );
}
