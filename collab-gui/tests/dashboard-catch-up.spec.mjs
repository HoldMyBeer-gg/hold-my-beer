// The dashboard must show what it missed.
//
// The feed used to hold only messages that arrived on the live stream while
// the page was listening. Anything sent while the app was closed, asleep, or
// suspended behind other windows (macOS does that to a covered window) was
// on the server and never on screen, and the roster and the "I'm here"
// check-in stayed stale until restart. These tests pin the catch-up: recent
// history is loaded whenever the stream connects and whenever the window
// becomes visible again.

import { test, expect } from '@playwright/test';

const SERVER = 'http://10.0.0.7:8000';
const joined = {
  joinOnly: true, setupComplete: true, token: 'tm_abc',
  serverUrl: SERVER, identity: 'Michael', teamName: 'crabs', projectDir: '',
};

const msg = (id, sender, recipient, content, t) => ({
  id, hash: `hash${id}`.padEnd(40, '0'), sender, recipient, content,
  timestamp: `2026-11-11T18:${t}Z`, refs: [],
});

async function mockTauri(page, history) {
  await page.addInitScript((init) => {
    window.__test = {
      config: init.config, calls: [], fetches: [],
      // What the server would return from /history/<name>.
      history: init.history,
    };
    const t = window.__test;
    window.fetch = async (url, opts = {}) => {
      const u = new URL(String(url));
      const method = (opts.method || 'GET').toUpperCase();
      t.fetches.push({ path: u.pathname, method });
      if (u.pathname.startsWith('/history/')) {
        const who = decodeURIComponent(u.pathname.slice('/history/'.length));
        return new Response(JSON.stringify(t.history[who] || []), { status: 200 });
      }
      if (u.pathname === '/events') {
        // A stream that stays open and says nothing, like a quiet team.
        return new Response(new ReadableStream({ start() {} }), { status: 200 });
      }
      return new Response('[]', { status: 200 });
    };
    window.__TAURI__ = {
      core: {
        invoke: async (cmd, args = {}) => {
          t.calls.push({ cmd, args });
          if (cmd === 'load_config') return t.config;
          if (cmd === 'server_running') return false;
          return null;
        },
      },
      event: { listen: async () => () => {} },
    };
  }, { config: joined, history });
}

const feedText = (page) => page.locator('#feed');
const count = (page, path, method) => page.evaluate(
  ([p, m]) => window.__test.fetches.filter(f => f.path === p && f.method === m).length,
  [path, method],
);

test('messages sent before the dashboard opened are shown', async ({ page }) => {
  await mockTauri(page, {
    Michael: [
      msg(1, 'rusty-deck-crab', 'Michael', 'Hello from the Deck', '54:31'),
      msg(2, 'rusty-win-crab', 'Michael', 'Please remove the port forward', '57:24'),
    ],
    all: [msg(3, 'hermi', 'all', 'bad-ass-mcp main moved', '58:04')],
  });
  await page.goto('/');

  await expect(feedText(page)).toContainText('Hello from the Deck');
  await expect(feedText(page)).toContainText('Please remove the port forward');
  await expect(feedText(page)).toContainText('bad-ass-mcp main moved');
  await expect(page.locator('#msg-count')).toHaveText('3 msgs');
});

test('history is shown oldest first, whatever order it arrives in', async ({ page }) => {
  await mockTauri(page, {
    Michael: [msg(2, 'rusty-win-crab', 'Michael', 'second', '57:24')],
    all: [msg(1, 'hermi', 'all', 'first', '53:00'), msg(3, 'hermi', 'all', 'third', '58:04')],
  });
  await page.goto('/');
  await expect(page.locator('#msg-count')).toHaveText('3 msgs');

  const text = await feedText(page).innerText();
  expect(text.indexOf('first')).toBeLessThan(text.indexOf('second'));
  expect(text.indexOf('second')).toBeLessThan(text.indexOf('third'));
});

test('a message is never shown twice', async ({ page }) => {
  // The same message is in both lists: sent by Michael to everyone.
  const shared = msg(7, 'Michael', 'all', 'Hello :)', '53:53');
  await mockTauri(page, { Michael: [shared], all: [shared] });
  await page.goto('/');

  await expect(feedText(page)).toContainText('Hello :)');
  await expect(page.locator('#msg-count')).toHaveText('1 msg');
});

test('coming back to the window catches up and checks in at once', async ({ page }) => {
  await mockTauri(page, { Michael: [msg(1, 'rusty-deck-crab', 'Michael', 'before', '54:31')] });
  await page.goto('/');
  await expect(feedText(page)).toContainText('before');

  const checkInsBefore = await count(page, '/presence/Michael', 'PUT');
  const rosterBefore = await count(page, '/roster', 'GET');

  // While the window was covered, two crabs wrote. Then it is shown again.
  await page.evaluate((extra) => {
    window.__test.history.Michael.push(...extra);
    document.dispatchEvent(new Event('visibilitychange'));
  }, [
    msg(2, 'rusty-macos-crab', 'Michael', 'while you were away', '57:47'),
    msg(3, 'rusty-kali-crab', 'Michael', 'nothing needed', '58:06'),
  ]);

  await expect(feedText(page)).toContainText('while you were away');
  await expect(feedText(page)).toContainText('nothing needed');
  await expect(page.locator('#msg-count')).toHaveText('3 msgs');
  expect(await count(page, '/presence/Michael', 'PUT')).toBeGreaterThan(checkInsBefore);
  expect(await count(page, '/roster', 'GET')).toBeGreaterThan(rosterBefore);
});

test('a history request that fails leaves the feed as it was', async ({ page }) => {
  await mockTauri(page, { Michael: [msg(1, 'rusty-deck-crab', 'Michael', 'kept', '54:31')] });
  await page.goto('/');
  await expect(feedText(page)).toContainText('kept');

  await page.evaluate(() => {
    const real = window.fetch;
    window.fetch = async (url, opts) => {
      if (String(url).includes('/history/')) throw new TypeError('Failed to fetch');
      return real(url, opts);
    };
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await page.waitForTimeout(300);

  await expect(feedText(page)).toContainText('kept');
  await expect(page.locator('#msg-count')).toHaveText('1 msg');
});
