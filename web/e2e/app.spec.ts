import { expect, test } from '@playwright/test';
import type { Locator, Page } from '@playwright/test';

const status = (page: Page): Locator => page.locator('.status strong');

function field(page: Page, label: string): Locator {
  return page.locator('.field', { has: page.locator('label', { hasText: new RegExp(`^${label}$`) }) }).locator('input[type=text]');
}

async function setField(page: Page, label: string, value: string): Promise<void> {
  const input = field(page, label);
  await input.fill(value);
  await input.press('Enter');
}

async function idle(page: Page): Promise<void> {
  await expect(page.locator('.status__busy')).toHaveCount(0, { timeout: 120_000 });
}

test.beforeEach(async ({ page }) => {
  const problems: string[] = [];
  page.on('pageerror', (error) => problems.push(error.message));
  await page.goto('./');
  await expect(status(page)).not.toHaveText('STANDBY', { timeout: 60_000 });
  expect(problems).toEqual([]);
});

test('synthetic bench locks on Orion and follows a slew', async ({ page }) => {
  await expect(status(page)).toHaveText('LOCK');
  await expect(page.locator('.data--stars')).toContainText('Rigel');
  await expect(page.locator('.data--quat tbody tr').first()).toContainText(/[+−]0\.\d{3} \d{3} \d{2}/);

  const before = await field(page, 'RA').inputValue();
  const canvas = page.locator('.sensor__canvas');
  const box = await canvas.boundingBox();
  if (!box) throw new Error('canvas has no layout box');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2 + 40, { steps: 6 });
  await page.mouse.up();
  await idle(page);
  expect(await field(page, 'RA').inputValue()).not.toBe(before);
  await expect(status(page)).toHaveText('LOCK');
});

test('a starved exposure loses the lock and a pipeline stage can be isolated', async ({ page }) => {
  // Orion survives 200 e- of read noise alone; a 5 ms exposure on top buries even Rigel.
  await setField(page, 'Read noise', '200');
  await setField(page, 'Exposure', '0.005');
  await idle(page);
  await expect(status(page)).toHaveText('NO LOCK');

  await setField(page, 'Read noise', '5');
  await setField(page, 'Exposure', '0.1');
  await idle(page);
  await expect(status(page)).toHaveText('LOCK');
  const centroids = page.locator('.strip__cell--button', { hasText: 'Centroids' });
  await centroids.click();
  await expect(centroids).toHaveAttribute('aria-pressed', 'true');
  await centroids.click();
  await expect(centroids).toHaveAttribute('aria-pressed', 'false');
});

test('a sequence acquires once and then tracks', async ({ page }) => {
  await setField(page, 'Across x', '0.5');
  await idle(page);
  await page.getByRole('button', { name: 'Run', exact: true }).click();
  await expect(status(page)).toHaveText('TRACK', { timeout: 30_000 });
  await expect(page.locator('.hint')).toContainText('First fix after 1 frame');
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
});

test('image bench solves the bundled frames, with and without a field of view', async ({ page }) => {
  await page.getByRole('button', { name: 'Image', exact: true }).click();

  await page.getByRole('button', { name: 'Synthetic tracker frame, Cygnus' }).click();
  await expect(status(page)).toHaveText('LOCK', { timeout: 60_000 });
  await idle(page);
  await expect(page.locator('.data--stars')).toContainText('Deneb');
  const fitted = page.locator('tr', { hasText: 'FOV fitted' }).locator('td');
  expect(Math.abs(Number.parseFloat(await fitted.innerText()) - 16)).toBeLessThan(0.1);

  await page.getByRole('button', { name: 'Orion and the Winter Triangle' }).click();
  await expect(page.locator('.data--stars')).toContainText('Sirius', { timeout: 60_000 });
  await idle(page);
  await expect(status(page)).toHaveText('LOCK');

  // A real frame from a wide meteor camera, solved with its equidistant lens.
  await page.getByRole('button', { name: 'Meteor camera, Arizona, 27 April 2023' }).click();
  await expect(page.locator('.data--stars')).toContainText('Arcturus', { timeout: 60_000 });
  await idle(page);
  await expect(status(page)).toHaveText('LOCK');
  const matched = await page.locator('tr', { hasText: 'Matched' }).locator('td').innerText();
  expect(Number.parseInt(matched, 10)).toBeGreaterThan(50);

  await page.getByRole('button', { name: 'Orion and the Winter Triangle' }).click();
  await expect(page.locator('.data--stars')).toContainText('Sirius', { timeout: 60_000 });
  await idle(page);

  // Forget the field of view: the ladder has to find 52 degrees from a 20 degree guess.
  await setField(page, 'Field of view', '20');
  await idle(page);
  await expect(status(page)).toHaveText('NO LOCK');
  await page.locator('.toggle', { hasText: 'Field of view unknown' }).click();
  await expect(page.locator('.search__text')).toBeVisible({ timeout: 30_000 });
  await idle(page);
  await expect(status(page)).toHaveText('LOCK');
  await expect(page.locator('.data--stars')).toContainText('Betelgeuse');
});

test('validation suites pass in the browser', async ({ page }) => {
  await page.getByRole('button', { name: 'Validation', exact: true }).click();
  const suites = page.locator('.suite');

  const wahba = suites.nth(1);
  await wahba.getByRole('button', { name: 'Run' }).click();
  await expect(wahba.locator('.verdict--pass')).toHaveCount(2, { timeout: 120_000 });

  const lis = suites.nth(2);
  await lis.locator('select').selectOption('100');
  await lis.getByRole('button', { name: 'Run' }).click();
  await expect(lis.locator('.progress__text')).toContainText('100 / 100 trials', { timeout: 170_000 });
  await expect(lis.locator('.verdict--pass')).toHaveCount(2);
  await expect(lis.locator('.verdict--fail')).toHaveCount(0);
  await expect(lis.locator('.caveat')).toContainText('upper bound');
});

test('a recorded sequence is tracked on the video bench and exported', async ({ page }) => {
  // Safari records MP4 and some WebKit builds record nothing; the bench then says so.
  test.skip(!(await page.evaluate(() => typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported('video/webm'))), 'this browser cannot record WebM');
  await setField(page, 'Across x', '0.4');
  await idle(page);
  await page.getByRole('button', { name: 'Record video' }).click();
  await expect(page.getByRole('button', { name: /Recording · (1[2-9]|[2-9]\d)/ })).toBeVisible({ timeout: 90_000 });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await page.getByRole('button', { name: 'Open on Video bench' }).click();

  const state = page.locator('.timeline__state');
  await expect(state).toHaveText('TRACK', { timeout: 120_000 });
  await idle(page);
  const counts = /^(\d+) \/ (\d+)\s/.exec(await page.locator('tr', { hasText: 'Frames solved' }).locator('td').innerText());
  const solved = Number(counts?.[1]);
  const total = Number(counts?.[2]);
  expect(total).toBeGreaterThanOrEqual(20);
  expect(solved / total).toBeGreaterThan(0.85);
  await expect(page.locator('tr', { hasText: 'FOV fitted' }).locator('td')).toContainText('14.0');

  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Attitude log CSV' }).click();
  const path = await (await download).path();
  const rows = (await (await import('node:fs/promises')).readFile(path, 'utf8')).trim().split('\n');
  expect(rows[0]).toContain('time_s,state,q1,q2,q3,q4,ra_deg');
  expect(rows).toHaveLength(total + 1);
  expect(rows.filter((row) => row.includes(',tracked,')).length).toBeGreaterThan(0.7 * total);

  // Clicking the timeline re-solves the frame behind that moment.
  const box = await page.locator('.timeline canvas').boundingBox();
  if (!box) throw new Error('timeline has no layout box');
  await page.mouse.click(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await expect(page.locator('.data--stars tbody tr').first()).toBeVisible();
  await expect(status(page)).toHaveText(/ACQUIRE|TRACK/);
});
test('design bench predicts the reference camera and the simulator agrees', async ({ page }) => {
  await page.getByRole('button', { name: 'Design', exact: true }).click();
  await expect(page.locator('.question')).toContainText('lens and detector');
  const prediction = page.locator('.panel--right .group').nth(0);
  const verification = page.locator('.panel--right .group').nth(1);
  const predicted = (label: string) => prediction.locator('tr', { hasText: label }).locator('td');
  await expect(predicted('Stars per field')).toHaveText(/^4\d\.\d$/);
  await expect(predicted('Streak limit')).toContainText('1.3');
  await expect(predicted('Sky with 5 or more')).toHaveText('100.0 %');

  await verification.locator('select').selectOption('50');
  await verification.getByRole('button', { name: 'Simulate and solve' }).click();
  await expect(verification.getByRole('button', { name: 'Simulate and solve' })).toBeVisible({ timeout: 120_000 });
  const cells = async (label: string) =>
    (await verification.locator('tr', { hasText: label }).locator('td').allInnerTexts()).map((text) => Number.parseFloat(text));
  const [, locks] = await cells('Locks');
  expect(locks).toBeGreaterThanOrEqual(96);
  const [predictedStars, measuredStars] = await cells('Stars detected');
  expect(Math.abs((measuredStars ?? 0) / (predictedStars ?? 1) - 1)).toBeLessThan(0.3);
  const [predictedCross, measuredCross] = await cells('Cross median');
  expect((measuredCross ?? 0) / (predictedCross ?? 1)).toBeGreaterThan(0.5);
  expect((measuredCross ?? 0) / (predictedCross ?? 1)).toBeLessThan(2);
  await expect(verification.locator('tr', { hasText: 'False locks' }).locator('td').nth(1)).toHaveText('0 / 50');

  // A long lens sees too little sky for the catalog.
  await setField(page, 'Focal length', '200');
  await expect(predicted('Sky with 5 or more')).toHaveText(/^\d\.\d %$/);
});

test('mission bench shows what blinds a camera along an orbit', async ({ page }) => {
  await page.getByRole('button', { name: 'Mission', exact: true }).click();
  await expect(page.locator('.question')).toContainText('how much of the time can the camera see stars');
  const span = page.locator('.panel--right .group').nth(0);
  const share = (label: string) => span.locator('tr', { hasText: label }).locator('td');
  // Looking away from the Earth, the Earth is never in the way.
  await expect(share('Blinded by the Earth')).toHaveText('0.0 %');
  await expect(share('Clear')).toHaveText(/^\d\d\.\d %$/);
  await expect(page.locator('.strip__cell', { hasText: 'Period' }).locator('.strip__value')).toHaveText(/^95.[67] min$/);

  // Along the orbit normal the limb is 23 degrees away, inside the 25 allowed.
  await setField(page, 'Tilt', '90');
  await expect(share('Blinded by the Earth')).toHaveText('100.0 %');
  await expect(span.locator('.notice')).toContainText('never has a clear view');
  await setField(page, 'Tilt', '60');
  await expect(share('Blinded by the Earth')).toHaveText('0.0 %');

  // A moment picked on the chart opens that sky on the Synthetic bench.
  await page.locator('.mission-chart__angles').click({ position: { x: 300, y: 100 } });
  const moment = page.locator('.panel--right .group').nth(1);
  await expect(moment.locator('tr', { hasText: 'Blinded by' }).locator('td')).toHaveText(/nothing|the Sun|the Moon/);
  await moment.getByRole('button', { name: 'Open this sky on Synthetic bench' }).click();
  await expect(page.locator('.question')).toContainText('find its attitude from one frame');
});

test('the manual states the purpose and lists every bench', async ({ page }) => {
  await page.getByRole('button', { name: 'Manual', exact: true }).click();
  await expect(page.locator('.manual__lede')).toContainText('which way a camera is pointing');
  await expect(page.locator('.manual__pipeline li')).toHaveCount(6);
  await expect(page.locator('.manual__table tbody tr')).toHaveCount(6);
  await expect(page.locator('.question')).toHaveCount(0);
  await page.getByRole('button', { name: 'Synthetic', exact: true }).click();
  await expect(page.locator('.question')).toContainText('find its attitude from one frame');
});
test('a mirrored image does not lock until it is flipped back', async ({ page }) => {
  await page.getByRole('button', { name: 'Image', exact: true }).click();
  await page.getByRole('button', { name: 'Synthetic tracker frame, Cygnus' }).click();
  await expect(status(page)).toHaveText('LOCK', { timeout: 60_000 });
  await idle(page);
  const mirrored = page.locator('.toggle', { hasText: 'Mirrored image' });
  await mirrored.click();
  await expect(status(page)).toHaveText('NO LOCK', { timeout: 60_000 });
  await idle(page);
  await mirrored.click();
  await expect(status(page)).toHaveText('LOCK', { timeout: 60_000 });
  await expect(page.locator('.data--stars')).toContainText('Deneb');
});

test('every bench fits a phone screen without sideways scrolling', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(status(page)).toHaveText('LOCK');
  for (const tab of ['Synthetic', 'Image', 'Video', 'Design', 'Mission', 'Validation', 'Manual']) {
    await page.getByRole('button', { name: tab, exact: true }).click();
    await expect(page.getByRole('button', { name: tab, exact: true })).toHaveAttribute('aria-current', 'page');
    const width = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(width, `${tab} is ${width} px wide`).toBeLessThanOrEqual(390);
  }
});
