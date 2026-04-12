import { test, expect } from '@playwright/test';

test.describe('setup wizard — step 1', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
  });

  test('page loads with wizard visible', async ({ page }) => {
    await expect(page).toHaveTitle('Hold My Beer');
    await expect(page.locator('#step-1')).toBeVisible();
    await expect(page.locator('#s1-token')).toBeVisible();
  });

  test('Generate button populates token field with 64 hex chars', async ({ page }) => {
    const input = page.locator('#s1-token');
    await expect(input).toHaveValue('');

    await page.getByRole('button', { name: 'Generate' }).click();

    const value = await input.inputValue();
    expect(value).toMatch(/^[0-9a-f]{64}$/);
    await expect(input).toHaveAttribute('type', 'text');
  });

  test('token field reverts to password type after 3s', async ({ page }) => {
    await page.getByRole('button', { name: 'Generate' }).click();
    await expect(page.locator('#s1-token')).toHaveAttribute('type', 'text');
    await page.waitForTimeout(3200);
    await expect(page.locator('#s1-token')).toHaveAttribute('type', 'password');
  });

  test('wizard scrolls when content exceeds viewport', async ({ page }) => {
    await page.setViewportSize({ width: 920, height: 500 });
    await page.goto('/');
    const result = await page.evaluate(() => {
      const w = document.getElementById('wizard');
      const wrap = document.querySelector('.wiz-steps-wrap');
      w.scrollTo(0, w.scrollHeight);
      return {
        overflows: w.scrollHeight > w.clientHeight,
        canScroll: getComputedStyle(w).overflowY === 'auto',
        bottomReachable: wrap.getBoundingClientRect().bottom <= window.innerHeight + 1,
      };
    });
    expect(result.overflows).toBe(true);
    expect(result.canScroll).toBe(true);
    expect(result.bottomReachable).toBe(true);
  });

  test('no console errors on load (other than favicon 404)', async ({ page }) => {
    const errors = [];
    page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
    page.on('pageerror', e => errors.push(e.message));
    await page.reload();
    await page.waitForTimeout(200);
    const real = errors.filter(e => !/favicon/.test(e));
    expect(real).toEqual([]);
  });
});
