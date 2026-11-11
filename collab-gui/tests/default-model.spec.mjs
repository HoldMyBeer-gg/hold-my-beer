// The model a new team starts with.
//
// Workers do real work on a codebase, so a new team defaults to the most
// capable model. Anyone who wants a cheaper one types it in.

import { test, expect } from '@playwright/test';

async function mockTauri(page) {
  await page.addInitScript(() => {
    window.__test = { config: {}, files: {} };
    const t = window.__test;
    window.fetch = async () => new Response('[]', { status: 200 });
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          switch (cmd) {
            case 'load_config':  return t.config;
            case 'save_config':  t.config = args.config; return null;
            case 'path_exists':  return false;
            case 'server_running': return false;
            default: return null;
          }
        },
      },
      event: { listen: async () => () => {} },
    };
  });
}

async function reachStep3(page) {
  await page.locator('#s1-url').fill('http://localhost:8000');
  await page.locator('#s1-team-name').fill('test-team');
  await page.locator('#s1-identity').fill('tester');
  await page.evaluate(() => window.__wizard.step1Next());
  await page.evaluate(() => {
    document.getElementById('s2-dir').value = '/tmp/proj';
    window.__wizard.cfg = { projectDir: '/tmp/proj' };
    window.__wizard.step2Next();
  });
}

test('a new team is offered opus', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await expect(page.locator('#s3-model')).toHaveValue('opus');
  expect(await page.evaluate(() => window.__wizard.cfg.model)).toBe('opus');
});

test('a model field left blank means opus', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await reachStep3(page);
  await page.locator('#s3-model').fill('');
  await page.evaluate(() => window.__wizard.step3Next());
  expect(await page.evaluate(() => window.__wizard.cfg.model)).toBe('opus');
});

test('a model that was typed in is kept', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await reachStep3(page);
  await page.locator('#s3-model').fill('haiku');
  await page.evaluate(() => window.__wizard.step3Next());
  expect(await page.evaluate(() => window.__wizard.cfg.model)).toBe('haiku');
});
