// Starting a new team when the server cannot start.
//
// The launch flow used to spawn the server, wait 1.2 seconds, and print
// "Server started" without looking. If the server had already died, most
// often because its port was taken, the next step failed with a bare
// "Failed to fetch" and no hint of the cause. These tests pin the flow to
// what the server actually did.

import { test, expect } from '@playwright/test';

const LOCAL = 'http://localhost:8000';

async function mockTauri(page, server = {}) {
  await page.addInitScript((init) => {
    window.__test = {
      files: {}, config: {}, calls: [], listeners: {},
      server: Object.assign({
        reachable: false,       // nothing answers before start_server
        answersAfterPolls: 0,   // 0 = as soon as it is spawned; -1 = never
        diesWith: null,         // a last log line; the process then exits
      }, init),
      polls: 0, spawned: false,
    };
    const t = window.__test;
    const emit = (name, payload) => (t.listeners[name] || []).forEach(fn => fn({ payload }));

    window.fetch = async (url, opts = {}) => {
      const path = new URL(String(url)).pathname;
      if (t.spawned && !t.server.diesWith && t.server.answersAfterPolls >= 0) {
        if (t.polls >= t.server.answersAfterPolls) t.server.reachable = true;
        t.polls += 1;
      }
      if (!t.server.reachable) throw new TypeError('Failed to fetch');
      if (path === '/admin/teams') {
        return new Response(JSON.stringify({ team_id: 't1', name: 'x', token: 'tm_minted' }), { status: 200 });
      }
      return new Response('[]', { status: 200 });
    };
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          t.calls.push({ cmd, args });
          switch (cmd) {
            case 'load_config':   return t.config;
            case 'save_config':   t.config = args.config; return null;
            case 'path_exists':   return Object.prototype.hasOwnProperty.call(t.files, args.path);
            case 'read_file':     return t.files[args.path];
            case 'write_file':    t.files[args.path] = args.content; return null;
            case 'start_server':
              t.spawned = true;
              if (t.server.diesWith) emit('server-log', t.server.diesWith);
              return null;
            // The Rust side clears this flag when the child process exits.
            case 'server_running': return t.spawned && !t.server.diesWith;
            case 'run_command':   return 0;
            default: return null;
          }
        },
      },
      event: {
        listen: async (name, fn) => {
          (t.listeners[name] = t.listeners[name] || []).push(fn);
          return () => {};
        },
      },
    };
  }, server);
}

async function launchNewTeam(page, url = LOCAL) {
  await page.locator('#s1-url').fill(url);
  await page.locator('#s1-team-name').fill('test-team');
  await page.locator('#s1-identity').fill('tester');
  await page.evaluate(() => window.__wizard.step1Next());
  await page.evaluate(() => {
    document.getElementById('s2-dir').value = '/tmp/proj';
    window.__wizard.cfg = { projectDir: '/tmp/proj' };
    window.__wizard.step2Next();
    window.__wizard.step3Next();
    window.__wizard.doLaunch();
  });
}

const commands = (page) => page.evaluate(() => window.__test.calls.map(c => c.cmd));

test('a server that dies on start is reported with its own last words', async ({ page }) => {
  await mockTauri(page, {
    diesWith: 'Error: Only one usage of each socket address is normally permitted. (os error 10048)',
  });
  await page.goto('/');
  await launchNewTeam(page);

  const log = page.locator('#launch-log');
  await expect(log).toContainText('The server did not stay running');
  await expect(log).toContainText('Only one usage of each socket address');
  await expect(log).toContainText('port 8000');
  await expect(log).not.toContainText('✓ Server started');
  await expect(log).not.toContainText('Failed to fetch');

  await expect(page.locator('#launch-btn')).toBeVisible();
  const cmds = await commands(page);
  expect(cmds).not.toContain('save_config');
  expect(cmds).not.toContain('run_command');
});

test('a server that never answers is reported, not assumed started', async ({ page }) => {
  await mockTauri(page, { answersAfterPolls: -1 });
  await page.goto('/');
  await launchNewTeam(page);

  const log = page.locator('#launch-log');
  await expect(log).toContainText('did not answer', { timeout: 15000 });
  await expect(log).not.toContainText('✓ Server started');
  expect(await commands(page)).not.toContain('save_config');
});

test('a server that takes a few seconds to come up is waited for', async ({ page }) => {
  await mockTauri(page, { answersAfterPolls: 4 });
  await page.goto('/');
  await launchNewTeam(page);

  await expect(page.locator('#launch-log')).toContainText('✓ Server started', { timeout: 15000 });
  await page.waitForFunction(() => window.__test.calls.some(c => c.cmd === 'save_config'), null, { timeout: 15000 });
  expect((await page.evaluate(() => window.__test.config)).token).toBe('tm_minted');
});

// A token typed while joining belongs to someone else's team. Carried into
// "Start a new team" it would be used as this new team's token.
test('switching between new team and join clears the secret fields', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');

  await page.locator('#s1-mode-join').check();
  await page.locator('#s1-token').fill('tm_someone_elses');
  await page.locator('#s1-code').fill('482913');
  await page.locator('#s1-url').fill('http://10.0.0.7:8000');
  await page.locator('#s1-identity').fill('deck');

  await page.locator('#s1-mode-new').check();
  await expect(page.locator('#s1-token')).toHaveValue('');
  // What is not secret stays, so nothing has to be retyped.
  await expect(page.locator('#s1-url')).toHaveValue('http://10.0.0.7:8000');
  await expect(page.locator('#s1-identity')).toHaveValue('deck');

  await page.locator('#s1-token').fill('aaaa');
  await page.locator('#s1-mode-join').check();
  await expect(page.locator('#s1-token')).toHaveValue('');
  await expect(page.locator('#s1-code')).toHaveValue('');
});
