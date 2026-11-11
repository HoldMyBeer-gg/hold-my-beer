// Every control has a name a screen reader can say.
//
// An input whose <label> has no `for`, or a button whose only content is a
// glyph, is announced as "text field" or "✕" — useless to a VoiceOver user
// and to automation (bad-ass-mcp finds controls by accessible name). These
// tests pin the names, using the same role + name lookup assistive tech uses.
//
// Like join-team.spec.mjs, this runs `public/index.html` in plain Chromium
// with `window.__TAURI__` and `fetch` replaced.

import { test, expect } from '@playwright/test';

const REMOTE = 'http://10.0.0.7:8000';

async function mockTauri(page, initial = {}) {
  await page.addInitScript((init) => {
    window.__test = { config: init.config || {}, roster: init.roster || [] };
    const t = window.__test;
    window.fetch = async (url) => {
      const path = new URL(String(url)).pathname;
      if (path === '/roster') return new Response(JSON.stringify(t.roster), { status: 200 });
      if (path === '/pair/codes') {
        return new Response(JSON.stringify({ code: '482913', expires_in: 300 }), { status: 200 });
      }
      return new Response('[]', { status: 200 });
    };
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          switch (cmd) {
            case 'load_config': return t.config;
            case 'save_config': t.config = args.config; return null;
            case 'path_exists': return false;
            case 'server_running': return false;
            case 'local_addresses': return [];
            default: return null;
          }
        },
      },
      event: { listen: async () => () => {} },
    };
  }, initial);
}

const joined = {
  joinOnly: true, setupComplete: true, token: 'tm_abc',
  serverUrl: REMOTE, identity: 'deck', teamName: 'pool', projectDir: '',
};

// Step 3 with the two default workers, without going through steps 1-2.
async function openStep3(page) {
  await mockTauri(page);
  await page.goto('/');
  await page.evaluate(() => {
    window.__wizard.cfg = { projectDir: '/tmp/proj' };
    document.getElementById('s2-dir').value = '/tmp/proj';
    window.__wizard.step2Next();
  });
}

test.describe('wizard', () => {
  test('the two setup modes are named by their titles alone', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await expect(page.getByRole('radio', { name: 'Start a new team', exact: true })).toBeVisible();
    await expect(page.getByRole('radio', { name: 'Join an existing team', exact: true })).toBeVisible();
  });

  test('the pairing code field is named', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await page.locator('#s1-mode-join').check();
    await expect(page.getByRole('textbox', { name: 'Pairing code', exact: true })).toBeVisible();
  });

  test('the project folder field is named', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await page.locator('#s1-team-name').fill('test-team');
    await page.locator('#s1-identity').fill('tester');
    await page.evaluate(() => window.__wizard.step1Next());
    await expect(page.getByRole('textbox', { name: 'Project folder', exact: true })).toBeVisible();
  });

  test('step 3 fields are named, per worker', async ({ page }) => {
    await openStep3(page);
    await expect(page.getByRole('combobox', { name: 'Default AI tool', exact: true })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'Model', exact: true })).toHaveCount(1);
    // Shown only for the Custom tool, and inside each worker's Advanced panel.
    const hidden = { exact: true, includeHidden: true };
    await expect(page.getByRole('textbox', { name: 'CLI template', ...hidden })).toHaveCount(1);

    await expect(page.getByRole('textbox', { name: 'Name of worker 1', exact: true })).toHaveValue('builder');
    await expect(page.getByRole('textbox', { name: 'Role of worker 1', exact: true })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'Name of worker 2', exact: true })).toHaveValue('reviewer');
    await expect(page.getByRole('textbox', { name: 'Role of worker 2', exact: true })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'Codebase path override for worker 1', ...hidden })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'Model override for worker 1', ...hidden })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'CLI template override for worker 1', ...hidden })).toHaveCount(1);
  });

  test('remove buttons say which worker they remove', async ({ page }) => {
    await openStep3(page);
    await expect(page.getByRole('button', { name: 'Remove worker builder', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Remove worker reviewer', exact: true })).toBeVisible();
  });

  test('the Advanced toggle reports whether it is open', async ({ page }) => {
    await openStep3(page);
    const toggle = page.getByRole('button', { name: 'Advanced settings for worker 1', exact: true });
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  });

  test('step errors are announced', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await expect(page.locator('#s1-error')).toHaveAttribute('role', 'alert');
  });
});

test.describe('dashboard', () => {
  test('icon buttons and text areas are named', async ({ page }) => {
    await mockTauri(page, { config: joined, roster: [{ instance_id: 'idler', role: 'Test worker' }] });
    await page.goto('/');
    await expect(page.locator('#dashboard')).toBeVisible();

    await expect(page.getByRole('button', { name: 'Setup', exact: true })).toHaveCount(1);
    await expect(page.getByRole('button', { name: 'Add a task', exact: true })).toHaveCount(1);
    await expect(page.getByRole('textbox', { name: 'Message', exact: true })).toBeVisible();
    // The task form is collapsed until the Todos panel and its + are opened.
    if (!(await page.locator('#todos-panel').isVisible())) {
      await page.getByRole('button', { name: 'Todos', exact: true }).click();
    }
    await page.getByRole('button', { name: 'Add a task', exact: true }).click();
    await expect(page.getByRole('textbox', { name: 'Task description', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Kick idler', exact: true })).toBeVisible();
  });

  test("a host's Stop All Workers is named without its ■ glyph", async ({ page }) => {
    const host = {
      setupComplete: true, token: 'tm_host', teamName: 'pool',
      serverUrl: 'http://localhost:8000', identity: 'lead', projectDir: '/tmp/proj',
    };
    await mockTauri(page, { config: host });
    await page.goto('/');
    await expect(page.locator('#dashboard')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop All Workers', exact: true })).toBeVisible();
  });

  test('toasts are announced', async ({ page }) => {
    await mockTauri(page, { config: joined });
    await page.goto('/');
    await expect(page.locator('#toasts')).toHaveAttribute('aria-live', 'assertive');
  });

  test('the pairing panel names its address, code and error', async ({ page }) => {
    await mockTauri(page, { config: joined });
    await page.goto('/');
    await expect(page.locator('#dashboard')).toBeVisible();
    await page.getByRole('button', { name: 'Add a machine', exact: true }).click();

    const dialog = page.getByRole('dialog', { name: 'Add a machine', exact: true });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('status', { name: 'Pairing code', exact: true })).toHaveText('482 913');
    await expect(dialog.getByRole('status', { name: 'Server address', exact: true })).toContainText(REMOTE);
    await expect(dialog.getByRole('button', { name: 'New code', exact: true })).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Done', exact: true })).toBeVisible();
    await expect(page.locator('#pair-error')).toHaveAttribute('role', 'alert');
  });
});
