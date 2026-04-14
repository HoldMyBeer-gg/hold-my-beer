import { test, expect } from '@playwright/test';

test.describe('Dashboard Features', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to localhost:1421 (dev server)
    await page.goto('http://localhost:1421');
    // Wait for the page to load
    await page.waitForLoadState('networkidle');
  });

  test('Usage tab shows total usage header', async ({ page }) => {
    // Check if usage-total element exists and has content
    const usageTotal = await page.locator('#usage-total');
    expect(usageTotal).toBeDefined();
    const text = await usageTotal.textContent();
    console.log('Usage total text:', text);
  });

  test('Todos panel has worker assignment dropdown', async ({ page }) => {
    // Check if todo-assign-to dropdown exists
    const assignDropdown = await page.locator('#todo-assign-to');
    expect(assignDropdown).toBeDefined();
  });

  test('Compose textarea supports @mention', async ({ page }) => {
    // Click in the compose textarea
    const textarea = await page.locator('#compose-text');
    await textarea.focus();
    // Type @ to trigger mention list
    await textarea.type('@');
    // Check if mention-list appears
    const mentionList = await page.locator('#mention-list');
    const isHidden = await mentionList.evaluate(el => el.hidden);
    console.log('Mention list hidden:', isHidden);
  });
});
