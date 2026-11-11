// Joining a team that already runs somewhere else.
//
// A second machine does not start a server, pick a project folder or define
// workers: it needs the server's address, the team token and a name, and
// then it shows the dashboard. These tests pin that flow, and the rule
// behind it: the GUI only ever starts a server for an address on this
// machine. It never answers "that server didn't respond" by quietly
// spawning a local one.
//
// Like wizard-logic.spec.mjs, this runs `public/index.html` in plain
// Chromium with `window.__TAURI__` and `fetch` replaced, so every
// `invoke(...)` and every request the page makes is recorded.

import { test, expect } from '@playwright/test';

const REMOTE = 'http://10.0.0.7:8000';
const LOCAL = 'http://localhost:8000';

async function mockTauri(page, initial = {}) {
  await page.addInitScript((init) => {
    window.__test = {
      files:  init.files  || {},
      config: init.config || {},
      calls:  [],
      fetches: [],
      // The pretend server. `reachable: false` makes every request fail the
      // way a refused connection or a timeout does.
      server: Object.assign(
        { reachable: true, rosterStatus: 200, rootStatus: 200, startsOnSpawn: true,
          redeemStatus: 200, codesStatus: 200, addresses: [] },
        init.server || {},
      ),
    };
    const t = window.__test;
    window.fetch = async (url, opts = {}) => {
      const u = String(url);
      t.fetches.push({ url: u, method: (opts.method || 'GET').toUpperCase() });
      if (!t.server.reachable) throw new TypeError('Failed to fetch');
      const path = new URL(u).pathname;
      if (path === '/roster') return new Response('[]', { status: t.server.rosterStatus });
      if (path === '/admin/teams') {
        return new Response(JSON.stringify({ team_id: 't1', name: 'x', token: 'tm_minted' }), { status: 200 });
      }
      if (path === '/pair/redeem') {
        t.redeemed = JSON.parse(opts.body || '{}');
        if (t.server.redeemStatus !== 200) return new Response('{}', { status: t.server.redeemStatus });
        return new Response(JSON.stringify({ token: 'tm_paired', team_id: 't1', team_name: 'pool' }), { status: 200 });
      }
      if (path === '/pair/codes') {
        t.codeAskedWith = (opts.headers || {}).Authorization;
        if (t.server.codesStatus !== 200) return new Response('{}', { status: t.server.codesStatus });
        return new Response(JSON.stringify({ code: '482913', expires_in: 300 }), { status: 200 });
      }
      if (path === '/') return new Response('{}', { status: t.server.rootStatus });
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
            case 'server_running': return false;
            case 'start_server':
              if (t.server.startsOnSpawn) t.server.reachable = true;
              return null;
            case 'run_command':   return 0;
            case 'local_addresses': return t.server.addresses;
            default: return null;
          }
        },
      },
      event: { listen: async () => () => {} },
    };
  }, initial);
}

const commands = (page) => page.evaluate(() => window.__test.calls.map(c => c.cmd));
const savedConfig = (page) => page.evaluate(() => window.__test.config);

async function fillJoin(page, { url = REMOTE, name = 'deck', token = 'tm_abc' } = {}) {
  await page.locator('#s1-mode-join').check();
  await page.locator('#s1-url').fill(url);
  await page.locator('#s1-identity').fill(name);
  await page.locator('#s1-token').fill(token);
}

// Drive the "new team" wizard to the launch step.
async function primeNewTeam(page, url) {
  await page.locator('#s1-url').fill(url);
  await page.locator('#s1-team-name').fill('test-team');
  await page.locator('#s1-identity').fill('tester');
  await page.evaluate(() => window.__wizard.step1Next());
  await page.evaluate(() => {
    document.getElementById('s2-dir').value = '/tmp/proj';
    window.__wizard.cfg = { projectDir: '/tmp/proj' };
    window.__wizard.step2Next();
    window.__wizard.step3Next();
  });
}

// ─── Step 1 in join mode ──────────────────────────────────────────────────────

test.describe('join an existing team', () => {
  test('choosing Join asks only for address, name and token', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');

    await expect(page.locator('#s1-mode-new')).toBeChecked();
    await expect(page.locator('#s1-team-name')).toBeVisible();
    await expect(page.locator('#btn-step1-next')).toContainText('Next');

    await page.locator('#s1-mode-join').check();

    await expect(page.locator('#s1-team-name')).toBeHidden();
    await expect(page.locator('#btn-generate-team-token')).toBeHidden();
    await expect(page.locator('#s1-url')).toBeVisible();
    await expect(page.locator('#s1-identity')).toBeVisible();
    await expect(page.locator('#s1-token')).toBeVisible();
    await expect(page.locator('#btn-step1-next')).toContainText('Join team');
  });

  test('a valid token opens the dashboard and starts nothing on this machine', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await fillJoin(page);
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#dashboard')).toBeVisible();
    await expect(page.locator('#wizard')).toBeHidden();

    const cmds = await commands(page);
    expect(cmds).toContain('save_config');
    expect(cmds).not.toContain('start_server');
    expect(cmds).not.toContain('run_command');
    expect(cmds).not.toContain('write_file');
    expect(cmds).not.toContain('mark_session_active');

    const cfg = await savedConfig(page);
    expect(cfg.joinOnly).toBe(true);
    expect(cfg.setupComplete).toBe(true);
    expect(cfg.serverUrl).toBe(REMOTE);
    expect(cfg.token).toBe('tm_abc');
    expect(cfg.identity).toBe('deck');

    // It checked the token against the team's own roster.
    const fetches = await page.evaluate(() => window.__test.fetches.map(f => f.url));
    expect(fetches).toContain(REMOTE + '/roster');
  });

  test('controls for a local server and local workers are hidden after joining', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await fillJoin(page);
    await page.locator('#btn-step1-next').click();
    await expect(page.locator('#dashboard')).toBeVisible();

    for (const id of ['btn-start-server', 'btn-stop-server', 'btn-start-workers', 'btn-stop-workers', 'btn-broadcast-stop']) {
      await expect(page.locator('#' + id)).toBeHidden();
    }
  });

  test('an address that does not answer is explained, and nothing is started', async ({ page }) => {
    await mockTauri(page, { server: { reachable: false } });
    await page.goto('/');
    await fillJoin(page);
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toBeVisible();
    await expect(page.locator('#s1-error')).toContainText("Can't reach " + REMOTE);
    await expect(page.locator('#wizard')).toBeVisible();
    await expect(page.locator('#dashboard')).toBeHidden();

    const cmds = await commands(page);
    expect(cmds).not.toContain('start_server');
    expect(cmds).not.toContain('save_config');
  });

  test('a token the server rejects is explained', async ({ page }) => {
    await mockTauri(page, { server: { rosterStatus: 401 } });
    await page.goto('/');
    await fillJoin(page);
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toContainText('did not accept that token');
    await expect(page.locator('#dashboard')).toBeHidden();
    expect(await commands(page)).not.toContain('save_config');
  });

  test('the token is required', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await fillJoin(page, { token: '' });
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toContainText('team token');
    const fetches = await page.evaluate(() => window.__test.fetches.length);
    expect(fetches).toBe(0);
  });

  test('the error clears when the mode changes', async ({ page }) => {
    await mockTauri(page, { server: { reachable: false } });
    await page.goto('/');
    await fillJoin(page);
    await page.locator('#btn-step1-next').click();
    await expect(page.locator('#s1-error')).toBeVisible();

    await page.locator('#s1-mode-new').check();
    await expect(page.locator('#s1-error')).toBeHidden();
  });
});

// ─── Opening the app again ────────────────────────────────────────────────────

test.describe('opening the app again', () => {
  const joined = {
    joinOnly: true, setupComplete: true, token: 'tm_abc',
    serverUrl: REMOTE, identity: 'deck', teamName: '', projectDir: '',
  };

  test('a joined machine goes straight to the dashboard without starting a server', async ({ page }) => {
    await mockTauri(page, { config: joined });
    await page.goto('/');

    await expect(page.locator('#dashboard')).toBeVisible();
    await expect(page.locator('#wizard')).toBeHidden();
    const cmds = await commands(page);
    expect(cmds).not.toContain('start_server');
    expect(cmds).not.toContain('mark_session_active');
    await expect(page.locator('#btn-start-workers')).toBeHidden();
  });

  test('a joined machine whose token was revoked returns to Join with the reason', async ({ page }) => {
    await mockTauri(page, { config: joined, server: { rosterStatus: 401 } });
    await page.goto('/');

    await expect(page.locator('#wizard')).toBeVisible();
    await expect(page.locator('#dashboard')).toBeHidden();
    await expect(page.locator('#s1-mode-join')).toBeChecked();
    await expect(page.locator('#s1-error')).toContainText('no longer accepts');
    await expect(page.locator('#s1-url')).toHaveValue(REMOTE);
    expect(await commands(page)).not.toContain('start_server');
  });

  test('a host whose server is already up does not start a second one', async ({ page }) => {
    await mockTauri(page, {
      config: {
        setupComplete: true, token: 'tm_abc', teamName: 'test-team',
        serverUrl: LOCAL, identity: 'lead', projectDir: '/tmp/proj',
      },
    });
    await page.goto('/');

    await expect(page.locator('#dashboard')).toBeVisible();
    expect(await commands(page)).not.toContain('start_server');
  });

  test('a host whose server is down starts it', async ({ page }) => {
    await mockTauri(page, {
      config: {
        setupComplete: true, token: 'tm_abc', teamName: 'test-team',
        serverUrl: LOCAL, identity: 'lead', projectDir: '/tmp/proj',
      },
      server: { reachable: false },
    });
    await page.goto('/');

    await expect(page.locator('#dashboard')).toBeVisible();
    expect(await commands(page)).toContain('start_server');
  });
});

// ─── Starting a new team ──────────────────────────────────────────────────────

test.describe('starting a new team', () => {
  test('another machine that does not answer is an error, not a local server', async ({ page }) => {
    await mockTauri(page, { server: { reachable: false, startsOnSpawn: false } });
    await page.goto('/');
    await primeNewTeam(page, REMOTE);
    await page.evaluate(() => window.__wizard.doLaunch());

    await expect(page.locator('#launch-log')).toContainText("Can't reach " + REMOTE);
    await expect(page.locator('#launch-log')).toContainText('localhost');
    expect(await commands(page)).not.toContain('start_server');
    expect((await savedConfig(page)).setupComplete).toBeFalsy();
  });

  test('this machine with no server running starts one', async ({ page }) => {
    await mockTauri(page, { server: { reachable: false } });
    await page.goto('/');
    await primeNewTeam(page, LOCAL);
    await page.evaluate(() => window.__wizard.doLaunch());

    await page.waitForFunction(() => window.__test.calls.some(c => c.cmd === 'save_config'));
    expect(await commands(page)).toContain('start_server');
    expect((await savedConfig(page)).token).toBe('tm_minted');
  });

  test('a server that is already there points to Join when no token was given', async ({ page }) => {
    await mockTauri(page, { server: { rootStatus: 401 } });
    await page.goto('/');
    await primeNewTeam(page, REMOTE);
    await page.evaluate(() => window.__wizard.doLaunch());

    await expect(page.locator('#launch-log')).toContainText('already running at ' + REMOTE);
    await expect(page.locator('#launch-log')).toContainText('Join an existing team');
    expect(await commands(page)).not.toContain('start_server');
  });
});

// ─── Which addresses are this machine ─────────────────────────────────────────

test('isLocalServerUrl recognises loopback addresses only', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  const check = (u) => page.evaluate((x) => window.__wizard.isLocalServerUrl(x), u);

  for (const u of ['http://localhost:8000', 'http://LOCALHOST:8000/', 'http://127.0.0.1:8000',
                   'http://127.1.2.3:9', 'http://[::1]:8000', 'http://0.0.0.0:8000']) {
    expect(await check(u), u).toBe(true);
  }
  for (const u of ['http://10.0.0.7:8000', 'http://192.168.1.5:8000', 'http://deck.tailnet.ts.net:8000',
                   'https://team.example.com', 'http://localhost.example.com:8000', 'not a url']) {
    expect(await check(u), u).toBe(false);
  }
});

// ─── Pairing codes ────────────────────────────────────────────────────────────
// Six digits from a machine already in the team, instead of the long token.

test.describe('joining with a pairing code', () => {
  async function fillCode(page, code, url = REMOTE) {
    await page.locator('#s1-mode-join').check();
    await page.locator('#s1-url').fill(url);
    await page.locator('#s1-identity').fill('deck');
    await page.locator('#s1-code').fill(code);
  }

  test('the code field belongs to Join only', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await expect(page.locator('#s1-code')).toBeHidden();
    await page.locator('#s1-mode-join').check();
    await expect(page.locator('#s1-code')).toBeVisible();
  });

  test('a valid code is traded for a token and opens the dashboard', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await fillCode(page, '482 913');
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#dashboard')).toBeVisible();
    expect(await page.evaluate(() => window.__test.redeemed)).toEqual({ code: '482913' });

    const cfg = await savedConfig(page);
    expect(cfg.token).toBe('tm_paired');
    expect(cfg.teamName).toBe('pool');
    expect(cfg.joinOnly).toBe(true);
    expect(cfg.serverUrl).toBe(REMOTE);
    expect(await commands(page)).not.toContain('start_server');
  });

  test('a code the server refuses is explained', async ({ page }) => {
    await mockTauri(page, { server: { redeemStatus: 404 } });
    await page.goto('/');
    await fillCode(page, '000000');
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toContainText('not valid');
    await expect(page.locator('#s1-error')).toContainText('new one');
    await expect(page.locator('#dashboard')).toBeHidden();
    expect(await commands(page)).not.toContain('save_config');
  });

  test('a code must be six digits before anything is sent', async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    await fillCode(page, '12345');
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toContainText('six digits');
    expect(await page.evaluate(() => window.__test.fetches.length)).toBe(0);
  });

  test('a server too old for pairing codes says to use the token', async ({ page }) => {
    await mockTauri(page, { server: { redeemStatus: 401 } });
    await page.goto('/');
    await fillCode(page, '482913');
    await page.locator('#btn-step1-next').click();

    await expect(page.locator('#s1-error')).toContainText('pairing codes');
    await expect(page.locator('#s1-error')).toContainText('team token');
  });

  test('an unreachable server is explained the same way as with a token', async ({ page }) => {
    await mockTauri(page, { server: { reachable: false } });
    await page.goto('/');
    await fillCode(page, '482913');
    await page.locator('#btn-step1-next').click();
    await expect(page.locator('#s1-error')).toContainText("Can't reach " + REMOTE);
  });
});

test.describe('adding a machine from the dashboard', () => {
  const joined = {
    joinOnly: true, setupComplete: true, token: 'tm_abc',
    serverUrl: REMOTE, identity: 'deck', teamName: 'pool', projectDir: '',
  };
  const host = {
    setupComplete: true, token: 'tm_host', teamName: 'pool',
    serverUrl: LOCAL, identity: 'lead', projectDir: '/tmp/proj',
  };

  test('shows a code and the address to type on the new machine', async ({ page }) => {
    await mockTauri(page, { config: joined });
    await page.goto('/');
    await expect(page.locator('#dashboard')).toBeVisible();
    await expect(page.locator('#pair-panel')).toBeHidden();

    await page.locator('#btn-add-machine').click();

    await expect(page.locator('#pair-panel')).toBeVisible();
    await expect(page.locator('#pair-code')).toHaveText('482 913');
    await expect(page.locator('#pair-address')).toContainText(REMOTE);
    await expect(page.locator('#pair-panel')).toContainText('5 minutes');
    expect(await page.evaluate(() => window.__test.codeAskedWith)).toBe('Bearer tm_abc');

    await page.locator('#btn-pair-close').click();
    await expect(page.locator('#pair-panel')).toBeHidden();
  });

  test("a host shows this machine's network addresses, never localhost", async ({ page }) => {
    await mockTauri(page, { config: host, server: { addresses: ['192.168.1.20', '100.64.0.5'] } });
    await page.goto('/');
    await expect(page.locator('#dashboard')).toBeVisible();
    await page.locator('#btn-add-machine').click();

    await expect(page.locator('#pair-address')).toContainText('http://192.168.1.20:8000');
    await expect(page.locator('#pair-address')).toContainText('http://100.64.0.5:8000');
    await expect(page.locator('#pair-address')).not.toContainText('localhost');
  });

  test('a host whose addresses are unknown says what to look for', async ({ page }) => {
    await mockTauri(page, { config: host });
    await page.goto('/');
    await page.locator('#btn-add-machine').click();

    await expect(page.locator('#pair-address')).toContainText("this machine's network address");
    await expect(page.locator('#pair-address')).toContainText(':8000');
    await expect(page.locator('#pair-address')).not.toContainText('localhost');
  });

  test('a refusal from the server is reported, and no stale code is shown', async ({ page }) => {
    await mockTauri(page, { config: joined, server: { codesStatus: 403 } });
    await page.goto('/');
    await page.locator('#btn-add-machine').click();

    await expect(page.locator('#pair-panel')).toBeVisible();
    await expect(page.locator('#pair-error')).toContainText('could not create a pairing code');
    await expect(page.locator('#pair-code')).toHaveText('');
  });
});
