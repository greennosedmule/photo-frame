import { type Page } from '@playwright/test';
import { expect, signIn, test } from './support/fixtures';
import { makeJpeg } from './support/images';

const PHOTOS = 45;
test.use({ photoCount: PHOTOS });
test.describe.configure({ mode: 'serial' });

async function openManage(page: Page) {
  await page.goto('/manage');
  await page.locator('.cell').first().waitFor();
}

const cells = (page: Page) => page.locator('.cell');
const heading = (page: Page) => page.locator('aside h2');

test('the grid is virtualised, newest first, and selection has every mode', async ({ page, stack }) => {
  await openManage(page);
  const m = await stack.manifest();
  expect(await cells(page).count()).toBeLessThan(m.photos.length);

  // Newest first by effective date.
  const newest = [...m.photos].sort((a, b) => b.effective_date.localeCompare(a.effective_date))[0]!;
  await expect(cells(page).first().locator('img')).toHaveAttribute('src', new RegExp(`/media/${newest.hash}/thumb`));

  await cells(page).nth(0).click();
  await expect(heading(page)).toHaveText(/^1 selected/);
  await cells(page).nth(5).click({ modifiers: ['Shift'] });
  await expect(heading(page)).toHaveText(/^6 selected/);
  await cells(page).nth(2).click({ modifiers: ['ControlOrMeta'] });
  await expect(heading(page)).toHaveText(/^5 selected/);
  await page.keyboard.press('Escape');
  await expect(heading(page)).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+a');
  await expect(heading(page)).toHaveText(new RegExp(`^${PHOTOS} selected`));
  await page.keyboard.press('Escape');

  // Rubber band across a 2x2 block of cells.
  const columns = await page.evaluate(() => {
    const tops = [...document.querySelectorAll<HTMLElement>('.cell')].map((c) => c.offsetTop);
    return tops.filter((t) => t === tops[0]).length;
  });
  const a = (await cells(page).nth(0).boundingBox())!;
  const b = (await cells(page).nth(columns + 1).boundingBox())!;
  await page.mouse.move(a.x + 40, a.y + 40);
  await page.mouse.down();
  await page.mouse.move(a.x + a.width + 20, a.y + 40, { steps: 5 });
  await page.mouse.move(b.x + 40, b.y + 40, { steps: 5 });
  await page.mouse.up();
  await expect(heading(page)).toHaveText(/^4 selected/);
});

test('tagging the selection needs no sign-in and shows checked and mixed states', async ({ page, stack }) => {
  await openManage(page);
  await cells(page).nth(0).click();
  await cells(page).nth(2).click({ modifiers: ['Shift'] });
  await expect(heading(page)).toHaveText(/^3 selected/);

  await page.getByPlaceholder('New tag').fill('Vacation');
  await page.getByRole('button', { name: 'Create tag and apply' }).click();
  await expect.poll(async () => (await stack.manifest()).tags.find((t) => t.name === 'Vacation')?.count).toBe(3);
  await expect(page.getByRole('checkbox', { name: /Vacation/ })).toBeChecked();

  // A selection where only some photographs have the tag shows the mixed state.
  await cells(page).nth(3).click({ modifiers: ['ControlOrMeta'] });
  await expect
    .poll(() => page.getByRole('checkbox', { name: /Vacation/ }).evaluate((c: HTMLInputElement) => c.indeterminate))
    .toBe(true);
});

test('a date override asks for sign-in, rejects a wrong password, then applies', async ({ page, stack }) => {
  await openManage(page);
  await cells(page).nth(0).click();
  await cells(page).nth(1).click({ modifiers: ['Shift'] });
  await page.locator('input[type="datetime-local"]').fill('1987-06-14T09:30');
  await page.getByRole('button', { name: 'Set', exact: true }).click();

  await expect(page.locator('.card h2')).toHaveText('Sign in to manage');
  await signIn(page, 'wrong');
  await expect(page.locator('.card .error')).toContainText('Wrong');
  await signIn(page);
  await expect(page.locator('.card')).toHaveCount(0);

  await expect
    .poll(async () => (await stack.manifest()).photos.filter((p) => p.date_source === 'override').length)
    .toBe(2);
  const overridden = (await stack.manifest()).photos.filter((p) => p.date_source === 'override');
  expect(overridden.map((p) => p.effective_date)).toEqual(['1987-06-14T09:30:00Z', '1987-06-14T09:30:00Z']);
  // The selection now shows the source, and originals were not touched.
  await expect(page.getByText('2 edited')).toBeVisible();
});

test('rotating the selection shows progress and regenerates thumbnails', async ({ page, stack }) => {
  await openManage(page);
  // One of the newest photographs, so its cell is inside the rendered window.
  const newest = [...(await stack.manifest()).photos].sort((x, y) => y.effective_date.localeCompare(x.effective_date));
  const target = newest.slice(0, 20).find((p) => p.rotation === 0)!;
  await page.locator(`.cell:has(img[src^="/media/${target.hash}/thumb"])`).click();
  await expect(heading(page)).toHaveText(/^1 selected/);
  await page.getByRole('button', { name: 'Rotate right' }).click();
  await signIn(page);

  await expect.poll(async () => (await stack.photo(target.hash))?.media_rotation).toBe(90);
  await expect(page.locator(`.cell img[src="/media/${target.hash}/thumb?r=90"]`)).toBeVisible();
  await expect(page.locator('.rotating')).toHaveCount(0);
  expect(await stack.derivativeFiles(target.hash)).toEqual(expect.arrayContaining([expect.stringContaining('-r90.jpg')]));
});

test('uploading indexes a new photograph and drops an exact duplicate', async ({ page, stack }) => {
  await openManage(page);
  const before = (await stack.manifest()).photos.length;
  const file = { name: 'uploaded.jpg', mimeType: 'image/jpeg', buffer: makeJpeg(400, 300, 4242) };

  await page.locator('input[type=file]').setInputFiles(file);
  await signIn(page);
  await expect(page.locator('.upload')).toContainText('received');
  await expect.poll(async () => (await stack.manifest()).photos.length).toBe(before + 1);
  expect((await stack.libraryFiles()).some((f) => f.endsWith('uploaded.jpg'))).toBe(true);
  await expect.poll(async () => (await stack.libraryFiles('incoming')).length).toBe(0);

  // Same bytes again: accepted, then recognised as a duplicate and removed.
  await page.locator('input[type=file]').setInputFiles({ ...file, name: 'again.jpg' });
  await expect(page.locator('.upload')).toContainText('received');
  await expect.poll(async () => (await stack.libraryFiles('incoming')).length).toBe(0);
  expect((await stack.manifest()).photos).toHaveLength(before + 1);
  expect((await stack.libraryFiles()).some((f) => f.endsWith('again.jpg'))).toBe(false);

  // A file that is not an image is refused, with a message and nothing written.
  await page.locator('input[type=file]').setInputFiles({ name: 'evil.jpg', mimeType: 'image/jpeg', buffer: Buffer.from('<html>not an image</html>') });
  await expect(page.locator('.upload')).toContainText(/not a supported image/i);
  expect(await stack.libraryFiles('incoming')).toHaveLength(0);
});

test('deleting asks first, hides at once, and removes originals and derivatives', async ({ page, stack }) => {
  await openManage(page);
  const before = await stack.manifest();
  await cells(page).nth(0).click();
  await cells(page).nth(1).click({ modifiers: ['Shift'] });
  const doomed = before.photos
    .sort((a, b) => b.effective_date.localeCompare(a.effective_date))
    .slice(0, 2)
    .map((p) => p.hash);
  const files = (await stack.libraryFiles()).length;

  await page.getByRole('button', { name: /Delete 2 photographs/ }).click();
  await expect(page.getByRole('alertdialog')).toContainText('Delete 2 photographs?');
  // Cancelling changes nothing.
  await page.getByRole('alertdialog').getByRole('button', { name: 'Cancel' }).click();
  expect((await stack.manifest()).photos).toHaveLength(before.photos.length);

  await page.getByRole('button', { name: /Delete 2 photographs/ }).click();
  await page.getByRole('alertdialog').getByRole('button', { name: /Delete 2 files/ }).click();
  await signIn(page);

  await expect.poll(async () => (await stack.manifest()).photos.length).toBe(before.photos.length - 2);
  await expect.poll(async () => (await stack.libraryFiles()).length).toBe(files - 2);
  for (const h of doomed) await expect.poll(() => stack.derivativeFiles(h)).toEqual([]);
});

test('export downloads the curation, including rotation, tags and date overrides', async ({ page, stack }) => {
  const photos = (await stack.manifest()).photos;
  const [a, b] = [photos[0]!, photos[1]!];
  // Tagging is open (the frame is a kiosk); rotation and dates need credentials.
  await fetch(`${stack.url}/api/photos/${a.hash}/tags`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ add: 'exported' }) });
  await stack.call('PATCH', `/api/photos/${b.hash}`, { rotate: 180, date_override: '2001-02-03T04:05:06Z' });

  await openManage(page);
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export curation' }).click();
  await signIn(page);
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/^curation-\d{8}T\d{6}Z\.json$/);
  const body = JSON.parse(await (await import('node:fs/promises')).readFile((await file.path())!, 'utf8'));

  expect(body.version).toBe(1);
  expect(body.tags).toEqual(expect.arrayContaining(['exported', 'Vacation']));
  expect(body.photos[a.hash].tags).toContain('exported');
  expect(body.photos[b.hash]).toMatchObject({ rotation: 180, date_override: '2001-02-03T04:05:06Z' });
  // Photographs with no curation are not listed.
  expect(Object.values(body.photos).every((p) => JSON.stringify(p) !== '{"favorite":false,"tags":[]}')).toBe(true);
});

test('the status panel reflects the library and can request a scan', async ({ page, stack }) => {
  await openManage(page);
  const total = (await stack.manifest()).photos.length;
  await expect(page.locator('.panel')).toContainText(`${total} photographs`);
  await expect(page.locator('.panel')).toContainText('Failed 0');
  await page.getByRole('button', { name: 'Scan now' }).click();
  await signIn(page);
  await expect(page.locator('.panel .note')).toHaveText('Scan requested.');
});
