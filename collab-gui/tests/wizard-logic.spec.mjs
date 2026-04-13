// Tests for the setup wizard's pure-JS logic and the bugs fixed in this PR:
//   1. parseWorkersYaml correctly reads the shape buildWorkersYaml writes
//   2. loadExistingProject populates wizard state from an existing workers.yaml
//   3. addWorker / removeWorker preserve in-progress edits to other cards
//   4. doLaunch does not silently overwrite an existing workers.yaml
//
// These run against `public/index.html` in plain Chromium. `window.__TAURI__`
// is replaced per-test with a shim that reads/writes a per-page state object,
// so every `invoke(...)` call in the frontend is observable and scriptable.

import { test, expect } from '@playwright/test';

// Install a __TAURI__ mock before the page script runs. Each page gets a
// fresh `window.__test` object; tests mutate it via `page.evaluate` to
// control what `invoke(...)` returns and to assert what the frontend called.
async function mockTauri(page, initial = {}) {
  await page.addInitScript((init) => {
    window.__test = {
      files:  init.files  || {},        // path -> string (read_file / write_file / path_exists)
      config: init.config || {},        // what load_config returns
      calls:  [],                       // every invoke(cmd, args) recorded in order
      runCommandExit: 0,                // what run_command resolves to
    };
    const t = window.__test;
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          t.calls.push({ cmd, args });
          switch (cmd) {
            case 'load_config':   return t.config;
            case 'save_config':   t.config = args.config; return null;
            case 'path_exists':   return Object.prototype.hasOwnProperty.call(t.files, args.path);
            case 'read_file':     {
              if (!(args.path in t.files)) throw new Error('ENOENT ' + args.path);
              return t.files[args.path];
            }
            case 'write_file':    t.files[args.path] = args.content; return null;
            case 'pick_directory': return t.pickResult || null;
            case 'home_dir':      return '/Users/test';
            case 'generate_token': return 'a'.repeat(64);
            case 'server_running': return false;
            case 'start_server':  return null;
            case 'stop_server':   return null;
            case 'run_command':   return t.runCommandExit;
            default: return null;
          }
        },
      },
      event: { listen: async () => () => {} },
    };
  }, initial);
}

// Helper: call a wizard function via the test bridge. The bridge exposes
// every function and stateful binding we need from the classic-script scope.
const callFn = (page, name, ...args) =>
  page.evaluate(({ n, a }) => window.__wizard[n](...a), { n: name, a: args });

// Helper: the step-2 dir input is readonly (populated only via the directory
// picker in real use), so fill it from JS instead of Playwright's .fill().
async function setProjectDir(page, dir) {
  await page.evaluate((d) => {
    document.getElementById('s2-dir').value = d;
    window.__wizard.cfg = { projectDir: d };
  }, dir);
}

// ─── parseWorkersYaml ─────────────────────────────────────────────────────────

test.describe('parseWorkersYaml', () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
  });

  test('parses the exact shape buildWorkersYaml emits', async ({ page }) => {
    const yaml = [
      'server: http://localhost:8000',
      'cli_template: "claude -p {prompt} --model {model}"',
      'model: haiku',
      'output_dir: ./workers',
      'codebase_path: /tmp/proj',
      'workers:',
      '  - name: builder',
      '    role: "Build features"',
      '  - name: reviewer',
      '    role: "Review code"',
      '',
    ].join('\n');
    const parsed = await page.evaluate((y) => window.__wizard.parseWorkersYaml(y), yaml);
    expect(parsed.server).toBe('http://localhost:8000');
    expect(parsed.cli_template).toBe('claude -p {prompt} --model {model}');
    expect(parsed.model).toBe('haiku');
    expect(parsed.workers).toHaveLength(2);
    expect(parsed.workers[0]).toEqual({ name: 'builder', role: 'Build features' });
    expect(parsed.workers[1]).toEqual({ name: 'reviewer', role: 'Review code' });
  });

  test('round-trips through buildWorkersYaml', async ({ page }) => {
    // Seed wizard state, build yaml, parse it, assert the important fields survive.
    const result = await page.evaluate(() => {
      window.__wizard.cfg = {
        serverUrl:   'http://localhost:8000',
        cliTemplate: 'claude -p {prompt}',
        model:       'sonnet',
        projectDir:  '/tmp/proj',
      };
      window.__wizard.workers = [
        { name: 'a', role: 'first' },
        { name: 'b', role: 'second with "quotes"' },
      ];
      const yaml   = window.__wizard.buildWorkersYaml();
      const parsed = window.__wizard.parseWorkersYaml(yaml);
      return { yaml, parsed };
    });
    expect(result.parsed.model).toBe('sonnet');
    expect(result.parsed.cli_template).toBe('claude -p {prompt}');
    expect(result.parsed.workers).toHaveLength(2);
    expect(result.parsed.workers[0].name).toBe('a');
    expect(result.parsed.workers[1].role).toContain('quotes');
  });
});

// ─── loadExistingProject ──────────────────────────────────────────────────────

test.describe('loadExistingProject', () => {
  test('populates wizard state when workers.yaml exists at the chosen dir', async ({ page }) => {
    await mockTauri(page, {
      files: {
        '/tmp/proj/workers.yaml': [
          'server: http://localhost:8000',
          'cli_template: "claude -p {prompt} --model {model} --allowedTools Bash,Read,Write,Edit"',
          'model: opus',
          'workers:',
          '  - name: alpha',
          '    role: "Alpha role"',
          '  - name: beta',
          '    role: "Beta role"',
          '  - name: gamma',
          '    role: "Gamma role"',
          '',
        ].join('\n'),
      },
    });
    await page.goto('/');
    const loaded = await callFn(page, 'loadExistingProject', '/tmp/proj');
    expect(loaded).toBe(true);

    const state = await page.evaluate(() => ({
      workers: window.__wizard.workers,
      model:   window.__wizard.cfg.model,
      tmpl:    window.__wizard.cfg.cliTemplate,
    }));
    expect(state.workers).toHaveLength(3);
    expect(state.workers.map(w => w.name)).toEqual(['alpha', 'beta', 'gamma']);
    expect(state.model).toBe('opus');

    // Worker cards should be rendered with the loaded values.
    const cardNames = await page.locator('.worker-card .wc-name').evaluateAll(
      (els) => els.map((e) => e.value)
    );
    expect(cardNames).toEqual(['alpha', 'beta', 'gamma']);
  });

  test('returns false and leaves state alone when the file is missing', async ({ page }) => {
    await mockTauri(page, { files: {} });
    await page.goto('/');
    const before = await page.evaluate(() => window.__wizard.workers.map(w => w.name));
    const loaded = await callFn(page, 'loadExistingProject', '/tmp/nothing-here');
    expect(loaded).toBe(false);
    const after = await page.evaluate(() => window.__wizard.workers.map(w => w.name));
    expect(after).toEqual(before);
  });
});

// ─── addWorker preserves edits (the redteamer bug) ────────────────────────────

test.describe('addWorker / removeWorker preserve in-progress edits', () => {
  test.beforeEach(async ({ page }) => {
    await mockTauri(page);
    await page.goto('/');
    // Advance the wizard to step 3 so worker cards are mounted.
    await page.locator('#s1-token').fill('x'.repeat(32));
    await page.locator('#s1-url').fill('http://localhost:8000');
    await page.locator('#s1-identity').fill('tester');
    await page.evaluate(() => window.__wizard.step1Next());
    await setProjectDir(page, '/tmp/proj');
    await page.evaluate(() => window.__wizard.step2Next());
  });

  test('clicking Add worker does not clobber typed edits on existing cards', async ({ page }) => {
    // Start state: builder, reviewer (defaults).
    const cards = page.locator('.worker-card');
    await expect(cards).toHaveCount(2);

    // Rename card 1 and card 2 by typing in the DOM inputs directly.
    await cards.nth(0).locator('.wc-name').fill('foo');
    await cards.nth(0).locator('.wc-role').fill('first role');
    await cards.nth(1).locator('.wc-name').fill('bar');
    await cards.nth(1).locator('.wc-role').fill('second role');

    // Add a third worker — this used to re-render from the stale `workers`
    // array and silently reset cards 1 & 2 back to builder/reviewer.
    await page.evaluate(() => window.__wizard.addWorker());
    await expect(cards).toHaveCount(3);

    const names = await cards.locator('.wc-name').evaluateAll(
      (els) => els.map((e) => e.value)
    );
    const roles = await cards.locator('.wc-role').evaluateAll(
      (els) => els.map((e) => e.value)
    );
    expect(names).toEqual(['foo', 'bar', '']);
    expect(roles).toEqual(['first role', 'second role', '']);
  });

  // Real-user flow: type character-by-character into two cards, then click
  // the actual "+ Add worker" button, then type into the new card. This is
  // the exact sequence the user hit that cost them a redteamer worker, and
  // uses `pressSequentially` + real button click rather than `.fill()` and
  // direct function calls, so timing/event differences vs the other tests
  // can't hide the bug.
  test('real-user flow: type-rename, click +Add, type new worker — all three stick', async ({ page }) => {
    const cards = page.locator('.worker-card');
    await expect(cards).toHaveCount(2);

    // Clear + type-rename card 1
    await cards.nth(0).locator('.wc-name').click();
    await cards.nth(0).locator('.wc-name').press('ControlOrMeta+a');
    await cards.nth(0).locator('.wc-name').pressSequentially('frontend');
    await cards.nth(0).locator('.wc-role').click();
    await cards.nth(0).locator('.wc-role').press('ControlOrMeta+a');
    await cards.nth(0).locator('.wc-role').pressSequentially('Build the UI');

    // Clear + type-rename card 2
    await cards.nth(1).locator('.wc-name').click();
    await cards.nth(1).locator('.wc-name').press('ControlOrMeta+a');
    await cards.nth(1).locator('.wc-name').pressSequentially('backend');
    await cards.nth(1).locator('.wc-role').click();
    await cards.nth(1).locator('.wc-role').press('ControlOrMeta+a');
    await cards.nth(1).locator('.wc-role').pressSequentially('Handle the API');

    // Click the real "+ Add worker" button, not the exposed function.
    await page.getByRole('button', { name: '+ Add worker' }).click();
    await expect(cards).toHaveCount(3);

    // Type into the brand new third card.
    await cards.nth(2).locator('.wc-name').click();
    await cards.nth(2).locator('.wc-name').pressSequentially('redteamer');
    await cards.nth(2).locator('.wc-role').click();
    await cards.nth(2).locator('.wc-role').pressSequentially('Red team the output');

    const names = await cards.locator('.wc-name').evaluateAll(
      (els) => els.map((e) => e.value)
    );
    const roles = await cards.locator('.wc-role').evaluateAll(
      (els) => els.map((e) => e.value)
    );
    expect(names).toEqual(['frontend', 'backend', 'redteamer']);
    expect(roles).toEqual(['Build the UI', 'Handle the API', 'Red team the output']);
  });

  test('removeWorker preserves edits on the remaining cards', async ({ page }) => {
    const cards = page.locator('.worker-card');
    await cards.nth(0).locator('.wc-name').fill('keeper');
    await cards.nth(0).locator('.wc-role').fill('kept role');
    // Remove card index 1 (the second one).
    await page.evaluate(() => window.__wizard.removeWorker(1));
    await expect(cards).toHaveCount(1);
    const name = await cards.nth(0).locator('.wc-name').inputValue();
    const role = await cards.nth(0).locator('.wc-role').inputValue();
    expect(name).toBe('keeper');
    expect(role).toBe('kept role');
  });
});

// ─── doLaunch does not silently overwrite an existing workers.yaml ────────────

test.describe('doLaunch overwrite guard', () => {
  // Must exactly match the bytes that buildWorkersYaml() emits — any drift
  // and the doLaunch overwrite-guard round-trip test will incorrectly report
  // a change and trigger a write. buildWorkersYaml quotes every scalar value
  // (YAML-injection hardening, commit 3ae56ee).
  const existingYaml = [
    'server: "http://localhost:8000"',
    'cli_template: "claude -p {prompt} --model {model} --allowedTools Bash,Read,Write,Edit"',
    'model: "haiku"',
    'output_dir: ./workers',
    'codebase_path: "/tmp/proj"',
    'workers:',
    '  - name: alpha',
    '    role: "Alpha role"',
    '  - name: beta',
    '    role: "Beta role"',
    '  - name: redteamer',
    '    role: "Red team"',
    '',
  ].join('\n');

  async function primeWizard(page) {
    await mockTauri(page, { files: { '/tmp/proj/workers.yaml': existingYaml } });
    await page.goto('/');
    await page.locator('#s1-token').fill('x'.repeat(32));
    await page.locator('#s1-url').fill('http://localhost:8000');
    await page.locator('#s1-identity').fill('tester');
    await page.evaluate(() => window.__wizard.step1Next());
    // Load the existing project (what doBrowse does after picking a dir).
    await setProjectDir(page, '/tmp/proj');
    await callFn(page, 'loadExistingProject', '/tmp/proj');
    await page.evaluate(() => window.__wizard.step2Next());
    await page.evaluate(() => window.__wizard.step3Next());
  }

  test('skips the write when wizard state round-trips to identical yaml', async ({ page }) => {
    await primeWizard(page);
    // Kick off the launch but do NOT wait — we only care about step 1's effect
    // on the file and the recorded calls. After a tick, invoke() for
    // write_file should not have been called.
    await page.evaluate(() => { window.__wizard.doLaunch(); });
    await page.waitForFunction(
      () => window.__test.calls.some(c => c.cmd === 'read_file' && c.args.path === '/tmp/proj/workers.yaml')
    );
    // Give the microtask after path_exists/read_file a tick to run.
    await page.waitForTimeout(50);
    const calls = await page.evaluate(() => window.__test.calls.map(c => c.cmd));
    expect(calls).toContain('path_exists');
    expect(calls).toContain('read_file');
    expect(calls).not.toContain('write_file');
  });

  test('writes the new yaml when wizard state differs from the file on disk', async ({ page }) => {
    await primeWizard(page);
    // Mutate wizard state: rename the first worker. This is exactly the
    // flow that produced "rename isn't taking" — the user edits a card,
    // clicks Launch, and expects the file to be updated.
    await page.evaluate(() => {
      window.__wizard.workers = window.__wizard.workers.map((w, i) =>
        i === 0 ? { ...w, name: 'webdev' } : w
      );
    });
    await page.evaluate(() => { window.__wizard.doLaunch(); });
    await page.waitForFunction(
      () => window.__test.calls.some(c => c.cmd === 'start_server')
    );
    // The file should reflect the rename.
    const contents = await page.evaluate(() => window.__test.files['/tmp/proj/workers.yaml']);
    expect(contents).toContain('name: webdev');
    expect(contents).not.toContain('name: alpha'); // the old name for index 0
  });
});
