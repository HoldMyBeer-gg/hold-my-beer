import { test } from '@playwright/test';
import lighthouse from 'lighthouse';
import * as chromeLauncher from 'chrome-launcher';
import fs from 'fs';
import path from 'path';

test.describe('Lighthouse Debug', () => {
  let chrome;
  let port;

  test.beforeAll(async () => {
    chrome = await chromeLauncher.launch({ chromeFlags: ['--headless', '--disable-gpu'] });
    port = chrome.port;
  });

  test.afterAll(async () => {
    if (chrome) {
      // Newer chrome-launcher exposes kill() on the launched instance, not
      // as a static helper. Old: chromeLauncher.kill(chrome). New: chrome.kill().
      //
      // Swallow errors: on Windows, chrome-launcher's tmp-dir cleanup races
      // with Chrome's own subprocess teardown and throws EPERM trying to
      // delete a file that's still held open. The cleanup is best-effort —
      // a leftover temp dir won't break anything and will get purged by the
      // OS eventually. Failing the test over it is pure noise.
      try { await chrome.kill(); } catch (e) { /* best-effort cleanup */ }
    }
  });

  test('Get detailed accessibility report', async () => {
    const options = {
      logLevel: 'error',
      port,
      onlyCategories: ['accessibility'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const lhr = runnerResult.lhr;

    const report = {
      score: lhr.categories.accessibility.score * 100,
      audits: {}
    };

    Object.values(lhr.audits).forEach((audit) => {
      if (audit.score !== null && audit.score < 1) {
        report.audits[audit.id] = {
          score: audit.score,
          title: audit.title,
          description: audit.description,
          details: audit.details ? {
            type: audit.details.type,
            items: audit.details.items ? audit.details.items.slice(0, 10) : []
          } : null
        };
      }
    });

    // Write to file for inspection
    fs.writeFileSync(
      path.join(process.cwd(), 'lighthouse-accessibility-report.json'),
      JSON.stringify(report, null, 2)
    );

    console.log('\n=== ACCESSIBILITY AUDIT REPORT ===');
    console.log('Score:', report.score);
    console.log('Failed Audits:', Object.keys(report.audits).length);
    Object.entries(report.audits).forEach(([id, audit]) => {
      console.log(`\n${id}:`);
      console.log(`  Title: ${audit.title}`);
      console.log(`  Score: ${audit.score}`);
      if (audit.details && audit.details.items.length > 0) {
        console.log(`  Issues: ${audit.details.items.length} items`);
        audit.details.items.slice(0, 3).forEach((item, idx) => {
          console.log(`    ${idx + 1}. ${JSON.stringify(item).substring(0, 100)}`);
        });
      }
    });

    console.log('\nFull report written to: lighthouse-accessibility-report.json');
  });

  test('Get detailed performance report', async () => {
    const options = {
      logLevel: 'error',
      port,
      onlyCategories: ['performance'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const lhr = runnerResult.lhr;

    const report = {
      score: lhr.categories.performance.score * 100,
      audits: {}
    };

    Object.values(lhr.audits).forEach((audit) => {
      if (audit.score !== null && audit.score < 1) {
        const items = audit.details ? (Array.isArray(audit.details.items) ? audit.details.items.slice(0, 10) : []) : [];
        report.audits[audit.id] = {
          score: audit.score,
          title: audit.title,
          description: audit.description,
          details: audit.details ? {
            type: audit.details.type,
            items: items,
            summary: audit.details.summary
          } : null
        };
      }
    });

    // Write to file for inspection
    fs.writeFileSync(
      path.join(process.cwd(), 'lighthouse-performance-report.json'),
      JSON.stringify(report, null, 2)
    );

    console.log('\n=== PERFORMANCE AUDIT REPORT ===');
    console.log('Score:', report.score);
    console.log('Failed Audits:', Object.keys(report.audits).length);
    Object.entries(report.audits).forEach(([id, audit]) => {
      console.log(`\n${id}:`);
      console.log(`  Title: ${audit.title}`);
      console.log(`  Score: ${audit.score}`);
      if (audit.details && audit.details.items.length > 0) {
        console.log(`  Issues: ${audit.details.items.length} items`);
        audit.details.items.slice(0, 3).forEach((item, idx) => {
          console.log(`    ${idx + 1}. ${JSON.stringify(item).substring(0, 100)}`);
        });
      }
    });

    console.log('\nFull report written to: lighthouse-performance-report.json');
  });
});
