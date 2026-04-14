import { test, expect } from '@playwright/test';
import lighthouse from 'lighthouse';
import * as chromeLauncher from 'chrome-launcher';

test.describe('Lighthouse Audits', () => {
  let chrome;
  let port;

  test.beforeAll(async () => {
    // Launch Chrome for lighthouse
    chrome = await chromeLauncher.launch({ chromeFlags: ['--headless', '--disable-gpu'] });
    port = chrome.port;
  });

  test.afterAll(async () => {
    if (chrome) {
      await chromeLauncher.killAll();
    }
  });

  test('Lighthouse Accessibility Audit - 100/100', async () => {
    const options = {
      logLevel: 'info',
      port,
      onlyCategories: ['accessibility'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const scores = runnerResult.lhr.categories;

    console.log('Accessibility Score:', scores.accessibility.score * 100);
    console.log('Accessibility Audits:');
    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.scoreDisplayMode === 'numeric' || audit.scoreDisplayMode === 'binary') {
        console.log(`  ${audit.id}: ${audit.score !== null ? audit.score : 'N/A'}`);
      }
    });

    // Extract accessibility details for debugging
    const details = {
      score: scores.accessibility.score,
      audits: {}
    };

    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.score !== null && audit.score < 1) {
        details.audits[audit.id] = {
          score: audit.score,
          title: audit.title,
          description: audit.description,
        };
      }
    });

    console.log('\nFailed Accessibility Audits:');
    console.log(JSON.stringify(details, null, 2));

    expect(scores.accessibility.score).toBe(1, 'Accessibility score should be 100/100');
  });

  test('Lighthouse Performance Audit - 100/100', async () => {
    const options = {
      logLevel: 'info',
      port,
      onlyCategories: ['performance'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const scores = runnerResult.lhr.categories;

    console.log('Performance Score:', scores.performance.score * 100);
    console.log('Performance Metrics:');
    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.scoreDisplayMode === 'numeric' || audit.scoreDisplayMode === 'binary') {
        console.log(`  ${audit.id}: ${audit.score !== null ? audit.score : 'N/A'}`);
      }
    });

    // Extract performance details for debugging
    const details = {
      score: scores.performance.score,
      audits: {}
    };

    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.score !== null && audit.score < 1) {
        details.audits[audit.id] = {
          score: audit.score,
          title: audit.title,
          description: audit.description,
        };
      }
    });

    console.log('\nFailed Performance Audits:');
    console.log(JSON.stringify(details, null, 2));

    expect(scores.performance.score).toBe(1, 'Performance score should be 100/100');
  });

  test('Lighthouse Best Practices Audit', async () => {
    const options = {
      logLevel: 'info',
      port,
      onlyCategories: ['best-practices'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const scores = runnerResult.lhr.categories;

    console.log('Best Practices Score:', scores['best-practices'].score * 100);
    console.log('Best Practices Audits:');
    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.scoreDisplayMode === 'numeric' || audit.scoreDisplayMode === 'binary') {
        console.log(`  ${audit.id}: ${audit.score !== null ? audit.score : 'N/A'}`);
      }
    });
  });

  test('Lighthouse SEO Audit', async () => {
    const options = {
      logLevel: 'info',
      port,
      onlyCategories: ['seo'],
      output: 'json',
    };

    const runnerResult = await lighthouse('http://localhost:1421', options);
    const scores = runnerResult.lhr.categories;

    console.log('SEO Score:', scores.seo.score * 100);
    console.log('SEO Audits:');
    Object.values(runnerResult.lhr.audits).forEach((audit) => {
      if (audit.scoreDisplayMode === 'numeric' || audit.scoreDisplayMode === 'binary') {
        console.log(`  ${audit.id}: ${audit.score !== null ? audit.score : 'N/A'}`);
      }
    });
  });
});
