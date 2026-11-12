// A machine that joined a team can add workers of its own.
//
// Joining used to be the end of the road: the machine could chat and watch,
// but only the machine that started the team could run workers. A pool of
// machines is the point of a team, so a joined machine can now pick a
// project folder, define workers, and start them against the team's server.
// It still never starts a server: that lives on the machine it joined.

import { test, expect } from '@playwright/test';

const SERVER = 'http://10.0.0.7:8000';
const joined = {
  joinOnly: true, setupComplete: true, token: 'tm_joined',
  serverUrl: SERVER, identity: 'deck', teamName: 'crabs', projectDir: '',
};

async function mockTauri(page, { config = joined, server = {} } = {}) {
  await page.addInitScript((init) => {
    window.__test = {
      config: init.config, files: {}, calls: [], fetches: [],
      server: Object.assign({
        reachable: true,
        team: 'crabs',            // what GET /team answers; null = old server (404)
        roster: [],               // instance ids already on the team
      }, init.server),
    };
    const t = window.__test;
    window.fetch = async (url, opts = {}) => {
      const u = new URL(String(url));
      const method = (opts.method || 'GET').toUpperCase();
      t.fetches.push({ path: u.pathname, method });
      if (!t.server.reachable) throw new TypeError('Failed to fetch');
      if (u.pathname === '/team') {
        if (t.server.team === null) return new Response('', { status: 404 });
        return new Response(JSON.stringify({ team_id: 't1', name: t.server.team }), { status: 200 });
      }
      if (u.pathname === '/roster') {
        const rows = t.server.roster.map(id => ({ instance_id: id, role: '', last_seen: new Date().toISOString() }));
        return new Response(JSON.stringify(rows), { status: 200 });
      }
      if (u.pathname === '/events') {
        return new Response(new ReadableStream({ start() {} }), { status: 200 });
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
            case 'server_running': return false;
            case 'run_command':   return 0;
            default: return null;
          }
        },
      },
      event: { listen: async () => () => {} },
    };
  }, { config, server });
}

const calls = (page, cmd) => page.evaluate(
  (c) => window.__test.calls.filter(x => x.cmd === c), cmd);
const visible = (page, id) => page.locator('#' + id);

// Walk the worker setup from the dashboard to a finished launch.
async function addWorkers(page, { dir = '/home/deck/proj', firstWorker = 'deck-builder' } = {}) {
  await page.locator('#btn-add-local-workers').click();
  await expect(page.locator('#step-2')).toHaveClass(/active/);
  // The folder field is filled by the Browse dialog, which is native. Set it
  // the way that dialog does.
  await page.evaluate((d) => { document.getElementById('s2-dir').value = d; }, dir);
  await page.locator('#btn-step2-next').click();
  await expect(page.locator('#step-3')).toHaveClass(/active/);
  await page.locator('.worker-card .wc-name').first().fill(firstWorker);
  await page.locator('#btn-step3-next').click();
}

test('a joined machine is offered workers of its own, and nothing to start yet', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');

  await expect(visible(page, 'btn-add-local-workers')).toBeVisible();
  await expect(visible(page, 'btn-start-workers')).toBeHidden();
  await expect(visible(page, 'btn-stop-workers')).toBeHidden();
  await expect(visible(page, 'btn-start-server')).toBeHidden();
  await expect(visible(page, 'btn-stop-server')).toBeHidden();
});

test('a machine that started its own team is not offered it', async ({ page }) => {
  await mockTauri(page, {
    config: { ...joined, joinOnly: false, serverUrl: 'http://localhost:8000', projectDir: '/p' },
  });
  await page.goto('/');
  await expect(visible(page, 'btn-start-workers')).toBeVisible();
  await expect(visible(page, 'btn-add-local-workers')).toBeHidden();
});

test('setup starts at the project folder, and Back returns to the dashboard', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await page.locator('#btn-add-local-workers').click();

  await expect(page.locator('#step-2')).toHaveClass(/active/);
  await expect(page.locator('#wizard')).toBeVisible();
  // The team is already chosen: step 1 is not somewhere to go back to.
  await page.locator('#step-2 [data-goto="1"]').click();
  await expect(page.locator('#dashboard')).toBeVisible();
  await expect(page.locator('#wizard')).toBeHidden();
  await expect(visible(page, 'btn-add-local-workers')).toBeVisible();
  // Nothing was saved by looking.
  expect(await calls(page, 'save_config')).toHaveLength(0);
});

test('launch writes team.yml for the joined team and starts workers, but no server', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await addWorkers(page);
  await expect(page.locator('#step-4')).toHaveClass(/active/);
  await page.locator('#launch-btn').click();

  await expect(page.locator('#dashboard')).toBeVisible({ timeout: 15000 });

  expect(await calls(page, 'start_server')).toHaveLength(0);

  const yaml = await page.evaluate(() => window.__test.files['/home/deck/proj/team.yml']);
  expect(yaml).toContain('team: "crabs"');
  expect(yaml).toContain(`server: "${SERVER}"`);
  expect(yaml).toContain('- name: deck-builder');

  const runs = await calls(page, 'run_command');
  expect(runs.map(r => r.args.args.join(' '))).toEqual(['init team.yml', 'start all']);
  for (const r of runs) {
    expect(r.args.cwd).toBe('/home/deck/proj');
    const env = Object.fromEntries(r.args.envs);
    expect(env.COLLAB_TOKEN).toBe('tm_joined');
    expect(env.COLLAB_SERVER).toBe(SERVER);
  }

  // Still a joined machine, now with a project.
  const saved = await page.evaluate(() => window.__test.config);
  expect(saved.joinOnly).toBe(true);
  expect(saved.projectDir).toBe('/home/deck/proj');
  expect(saved.token).toBe('tm_joined');
  expect(saved.serverUrl).toBe(SERVER);

  // The quit prompt has to know there are workers to stop.
  const active = await calls(page, 'mark_session_active');
  expect(active.at(-1).args.projectDir).toBe('/home/deck/proj');
});

test('after adding workers the dashboard controls them, and still no server buttons', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await addWorkers(page);
  await page.locator('#launch-btn').click();
  await expect(page.locator('#dashboard')).toBeVisible({ timeout: 15000 });

  await expect(visible(page, 'btn-start-workers')).toBeVisible();
  await expect(visible(page, 'btn-stop-workers')).toBeVisible();
  await expect(visible(page, 'btn-add-local-workers')).toBeHidden();
  await expect(visible(page, 'btn-start-server')).toBeHidden();
  await expect(visible(page, 'btn-stop-server')).toBeHidden();
});

test('reopening the app on a joined machine with workers', async ({ page }) => {
  await mockTauri(page, { config: { ...joined, projectDir: '/home/deck/proj' } });
  await page.goto('/');

  await expect(page.locator('#dashboard')).toBeVisible();
  await expect(visible(page, 'btn-start-workers')).toBeVisible();
  await expect(visible(page, 'btn-add-local-workers')).toBeHidden();
  await expect(visible(page, 'btn-start-server')).toBeHidden();
  expect(await calls(page, 'start_server')).toHaveLength(0);
  const active = await calls(page, 'mark_session_active');
  expect(active).toHaveLength(1);
  expect(active[0].args.projectDir).toBe('/home/deck/proj');
});

test('a machine that joined by token learns the team name from the server', async ({ page }) => {
  await mockTauri(page, { config: { ...joined, teamName: '' }, server: { team: 'crabs' } });
  await page.goto('/');
  await addWorkers(page);
  await page.locator('#launch-btn').click();
  await expect(page.locator('#dashboard')).toBeVisible({ timeout: 15000 });

  const yaml = await page.evaluate(() => window.__test.files['/home/deck/proj/team.yml']);
  expect(yaml).toContain('team: "crabs"');
  expect((await page.evaluate(() => window.__test.config)).teamName).toBe('crabs');
});

test('when the team name cannot be learned, setup says why and does not start', async ({ page }) => {
  await mockTauri(page, { config: { ...joined, teamName: '' }, server: { team: null } });
  await page.goto('/');
  await page.locator('#btn-add-local-workers').click();

  await expect(page.locator('.toast').last()).toContainText('which team');
  await expect(page.locator('#dashboard')).toBeVisible();
  await expect(page.locator('#wizard')).toBeHidden();
});

test('a worker name someone on the team already uses is refused before launch', async ({ page }) => {
  await mockTauri(page, { server: { roster: ['builder', 'hermi'] } });
  await page.goto('/');
  await addWorkers(page, { firstWorker: 'hermi' });

  await expect(page.locator('.toast').last()).toContainText('hermi');
  await expect(page.locator('.toast').last()).toContainText('already');
  await expect(page.locator('#step-3')).toHaveClass(/active/);
  expect(await calls(page, 'write_file')).toHaveLength(0);
});

test('an unreachable team server is reported in terms of the team, not of joining', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await addWorkers(page);
  await page.evaluate(() => { window.__test.server.reachable = false; });
  await page.locator('#launch-btn').click();

  const log = page.locator('#launch-log');
  await expect(log).toContainText("Can't reach");
  await expect(log).toContainText(SERVER);
  await expect(log).not.toContainText('Join an existing team');
  await expect(log).not.toContainText('localhost:8000');
  expect(await calls(page, 'start_server')).toHaveLength(0);
  expect(await calls(page, 'run_command')).toHaveLength(0);
});

// Words that were written for the machine that hosts the team.
test('a joined machine is not told it is starting a server', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await expect(page.locator('#feed-empty')).not.toContainText('start the server');
  await expect(page.locator('#feed-empty')).toContainText('No messages yet');

  await addWorkers(page);
  await expect(page.locator('#step-4')).toHaveClass(/active/);
  await expect(page.locator('#li-server .li-label')).toHaveText("Connecting to the team's server");
});

test('a machine starting its own team still reads as before', async ({ page }) => {
  await mockTauri(page, { config: {} });
  await page.goto('/');
  await page.locator('#s1-url').fill('http://localhost:8000');
  await page.locator('#s1-team-name').fill('test-team');
  await page.locator('#s1-identity').fill('tester');
  await page.locator('#btn-step1-next').click();
  await expect(page.locator('#step-2')).toHaveClass(/active/);
  await expect(page.locator('#li-server .li-label')).toHaveText('Starting collab-server');
  await expect(page.locator('#feed-empty')).toContainText('start the server');
});

// A step that is hidden while one of its fields holds focus leaves focus on
// nothing. Assistive tools follow focus, so each step change moves it to the
// heading of the step now on screen.
test('changing step moves focus to the new step heading', async ({ page }) => {
  await mockTauri(page);
  await page.goto('/');
  await page.locator('#btn-add-local-workers').click();
  await expect(page.locator('#step-2')).toHaveClass(/active/);
  await expect(page.locator('#step-2 .step-title')).toBeFocused();

  await page.evaluate(() => { document.getElementById('s2-dir').value = '/home/deck/proj'; });
  await page.locator('#btn-step2-next').click();
  await expect(page.locator('#step-3 .step-title')).toBeFocused();

  // Leave focus in a field of the step that is about to be hidden.
  await page.locator('.worker-card .wc-name').first().fill('deck-builder');
  await page.locator('.worker-card .wc-name').first().focus();
  await page.locator('#btn-step3-next').click();
  await expect(page.locator('#step-4')).toHaveClass(/active/);
  await expect(page.locator('#step-4 .step-title')).toBeFocused();
});
