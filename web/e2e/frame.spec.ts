import { currentSrc, expect, hashOf, overlayShown, settingsInIndexedDb, signIn, test } from './support/fixtures';

test.describe.configure({ mode: 'serial' });

/** Load the frame and wait until a photograph is showing. */
async function openFrame(page: import('@playwright/test').Page) {
  await page.goto('/');
  await page.locator('.slide img.photo').first().waitFor();
  await page.waitForTimeout(600);
}

/** Open the overlay if it is not already showing, and confirm it really is. */
async function ensureOverlay(page: import('@playwright/test').Page) {
  if (!(await overlayShown(page))) {
    const { x, y } = await surfacePoint(page);
    await page.mouse.click(x, y);
    expect(await overlayShown(page)).toBe(true);
  }
}

async function surfacePoint(page: import('@playwright/test').Page) {
  const box = (await page.locator('.surface').boundingBox())!;
  return { x: box.x + box.width / 2, y: box.y + box.height / 3, box };
}

test('a click toggles the overlay, and right-click shows it instead of the browser menu', async ({ page }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);

  await page.mouse.click(x, y);
  expect(await overlayShown(page)).toBe(true);
  await page.mouse.click(x, y);
  expect(await overlayShown(page)).toBe(false);

  // The browser's own menu must not open: the event is cancelled on the photo.
  const cancelled = await page.evaluate(() => {
    const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    document.querySelector('.surface')!.dispatchEvent(e);
    return e.defaultPrevented;
  });
  expect(cancelled).toBe(true);
  await page.mouse.click(x, y, { button: 'right' });
  expect(await overlayShown(page)).toBe(true);
});

test('holding the mouse down opens nothing, and overlay buttons open the two sheets', async ({ page }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);

  // Long press is for touch and pen; a held mouse button is a drag.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.waitForTimeout(900);
  await page.mouse.up();
  await expect(page.locator('.sheet')).toHaveCount(0);

  await ensureOverlay(page);
  await page.getByRole('button', { name: 'Frame settings' }).click();
  await expect(page.locator('.sheet h2')).toHaveText('Frame settings');
  await page.locator('.sheet header button').click();
  await expect(page.locator('.sheet')).toHaveCount(0);

  await ensureOverlay(page);
  await page.getByRole('button', { name: 'This photograph' }).click();
  await expect(page.locator('.sheet h2')).toHaveText('This photograph');
});

test('the photograph follows the finger during a drag and springs back when released short', async ({ page }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);
  const first = await currentSrc(page);
  const slideX = () => page.evaluate(() => {
    const el = [...document.querySelectorAll<HTMLElement>('.slide')].find((s) => s.getAttribute('aria-hidden') === 'false')!;
    return el.getBoundingClientRect().x;
  });

  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 120, y, { steps: 6 });
  await expect.poll(slideX).toBeLessThan(-60);
  await page.mouse.move(x - 60, y, { steps: 3 }); // slow, short: no swipe
  await page.waitForTimeout(400);
  await page.mouse.up();
  await expect.poll(slideX).toBe(0);
  expect(await currentSrc(page)).toBe(first);
});

test('swipes, arrow keys, space and f drive the frame', async ({ page, stack }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);
  const first = await currentSrc(page);

  // Dragging left goes forward, right comes back, and neither toggles the overlay.
  await page.mouse.move(x + 200, y + 100);
  await page.mouse.down();
  await page.mouse.move(x - 200, y + 100, { steps: 6 });
  await page.mouse.up();
  await expect.poll(() => currentSrc(page)).not.toBe(first);
  await page.mouse.move(x - 200, y + 100);
  await page.mouse.down();
  await page.mouse.move(x + 200, y + 100, { steps: 6 });
  await page.mouse.up();
  await expect.poll(() => currentSrc(page)).toBe(first);
  expect(await overlayShown(page)).toBe(false);

  await page.keyboard.press('ArrowRight');
  await expect.poll(() => currentSrc(page)).not.toBe(first);
  await page.keyboard.press('ArrowLeft');
  await expect.poll(() => currentSrc(page)).toBe(first);

  await page.keyboard.press(' ');
  await expect(page.locator('.paused')).toBeVisible();

  const favourites = async () => (await stack.manifest()).photos.filter((p) => p.favorite).length;
  const before = await favourites();
  await page.keyboard.press('f');
  await expect.poll(favourites).toBe(before + 1);
  await page.keyboard.press('f');
  await expect.poll(favourites).toBe(before);
});

test('frame settings live in IndexedDB only and survive a reload', async ({ page, stack }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);
  await ensureOverlay(page);
  await page.getByRole('button', { name: 'Frame settings' }).click();
  await page.locator('.sheet select').first().selectOption('chronological');

  await expect.poll(async () => (await settingsInIndexedDb(page))?.ordering).toBe('chronological');
  expect(await page.evaluate(() => Object.keys(localStorage))).toEqual([]);

  await page.reload();
  await page.locator('.slide img.photo').first().waitFor();
  const oldest = [...(await stack.manifest()).photos].sort((a, b) => a.effective_date.localeCompare(b.effective_date))[0]!;
  await expect.poll(async () => hashOf(await currentSrc(page))).toBe(oldest.hash);
});

test('tags are shared, hiding is this frame only', async ({ page, stack }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);
  const before = (await stack.manifest()).photos.length;
  const hash = hashOf(await currentSrc(page));

  await ensureOverlay(page);
  await page.getByRole('button', { name: 'This photograph' }).click();
  await page.getByPlaceholder('New tag').fill('e2e-tag');
  await page.getByRole('button', { name: 'Create tag' }).click();
  await expect.poll(async () => (await stack.photo(hash))?.tags).toEqual(['e2e-tag']);

  await page.getByRole('button', { name: 'Hide this photo' }).click();
  await expect(page.locator('.sheet')).toHaveCount(0);
  await expect.poll(async () => hashOf(await currentSrc(page))).not.toBe(hash);
  // Hiding never reaches the server: every frame's library is unchanged.
  expect((await stack.manifest()).photos).toHaveLength(before);
  await expect.poll(async () => (await settingsInIndexedDb(page))?.hidden).toEqual([hash]);

  await ensureOverlay(page);
  await page.getByRole('button', { name: 'Frame settings' }).click();
  await page.getByRole('button', { name: /Unhide 1 hidden photo/ }).click();
  await expect.poll(async () => (await settingsInIndexedDb(page))?.hidden).toEqual([]);
});

test('rotating a photograph regenerates it under a new URL and leaves the original alone', async ({ page, stack }) => {
  await openFrame(page);
  const { x, y } = await surfacePoint(page);
  const before = await currentSrc(page);
  const hash = hashOf(before);
  const natural = () =>
    page.evaluate(() => {
      const slide = [...document.querySelectorAll<HTMLElement>('.slide')].find((s) => s.style.opacity === '1');
      const img = slide!.querySelector('img.photo') as HTMLImageElement;
      return { w: img.naturalWidth, h: img.naturalHeight };
    });
  const upright = await natural();
  const originals = await stack.libraryFiles();

  await ensureOverlay(page);
  await page.getByRole('button', { name: 'This photograph' }).click();
  await page.getByRole('button', { name: 'Rotate right' }).click();

  // Curation edits need credentials: a login form appears rather than a browser dialog.
  await expect(page.locator('.card h2')).toHaveText('Sign in to manage');
  await signIn(page, 'wrong');
  await expect(page.locator('.card .error')).toBeVisible();
  await signIn(page);

  await expect.poll(async () => (await stack.photo(hash))?.media_rotation).toBe(90);
  expect((await stack.photo(hash))?.rotation).toBe(90);
  await expect.poll(() => currentSrc(page)).toBe(`${before}?r=90`);
  await expect.poll(natural).toEqual({ w: upright.h, h: upright.w });
  expect(await stack.derivativeFiles(hash)).toEqual(expect.arrayContaining([expect.stringContaining('-r90.jpg')]));
  expect(await stack.libraryFiles()).toEqual(originals); // never modified

  // Turning back removes the rotated files once the database no longer points at them.
  await page.getByRole('button', { name: 'Rotate left' }).click();
  await expect.poll(async () => (await stack.photo(hash))?.media_rotation).toBe(0);
  await expect.poll(async () => (await stack.derivativeFiles(hash)).some((f) => f.includes('-r90'))).toBe(false);
});

test('a frame with cached photographs keeps running with no network', async ({ page, context, stack }) => {
  const total = (await stack.manifest()).photos.length;
  await page.goto('/');
  await page.evaluate(() => navigator.serviceWorker.ready);
  await page.reload(); // now controlled by the service worker, so requests are cached
  await page.locator('.slide img.photo').first().waitFor();

  // The whole display set (and its blur backdrops) is cached, not just what was shown.
  const cached = () =>
    page.evaluate(async () => {
      const keys = await (await caches.open('media')).keys();
      return keys.filter((r) => r.url.includes('/display') || r.url.includes('/blur')).length;
    });
  await expect.poll(cached, { timeout: 60_000 }).toBeGreaterThanOrEqual(total * 2);

  await context.setOffline(true);
  await page.reload();
  const shown = page.locator('.slide img.photo').first();
  await shown.waitFor();
  await expect.poll(() => shown.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
  // Moving on works too: the next photograph comes from the cache as well.
  const first = await currentSrc(page);
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => currentSrc(page)).not.toBe(first);
  await expect
    .poll(() =>
      page.evaluate(() => {
        const slide = [...document.querySelectorAll<HTMLElement>('.slide')].find((s) => s.style.opacity === '1');
        const img = slide!.querySelector('img.photo') as HTMLImageElement;
        return img.complete && img.naturalWidth > 0;
      }),
    )
    .toBe(true);
  await context.setOffline(false);
});
