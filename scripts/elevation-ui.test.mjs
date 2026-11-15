// Browser plugin unavailable; isolated Playwright IPC fixtures never open real UAC.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';

test('ordinary startup, explicit elevation, cancellation, background rules and section restoration', { timeout: 90000 }, async () => {
  const fixture = fileURLToPath(new URL('./fixtures/elevation-ui.js', import.meta.url)).replaceAll('\\', '/');
  const server = await createServer({ server: { port: 0, strictPort: false }, plugins: [{ name: 'elevation-test', configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if (!req.url?.startsWith('/elevation-test.html')) return next();
      try {
        const html = (await readFile(new URL('../src/index.html', import.meta.url), 'utf8')).replace('src="/main.js"', `src="/@fs/${fixture}"`);
        res.setHeader('Content-Type', 'text/html'); res.end(await server.transformIndexHtml(req.url, html));
      } catch (error) { next(error); }
    });
  } }] });
  let browser;
  try {
    await server.listen(); browser = await chromium.launch({ channel: 'msedge', headless: true });
    await mkdir('src-tauri/target/elevation-qa', { recursive: true });
    for (const [theme, lang, width, height] of [['zapret','ru',1100,850], ['light','en',900,750], ['graphite','ru',900,750]]) {
      const page = await browser.newPage({ viewport: { width, height }, reducedMotion: 'reduce' });
      const errors = []; page.on('pageerror', error => errors.push(String(error)));
      await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?theme=${theme}&lang=${lang}`);
      await page.waitForFunction(() => !document.documentElement.classList.contains('app-loading'));
      await page.locator('#section-home').waitFor({ state: 'visible' });
      assert.equal(await page.locator('html').getAttribute('data-theme'), theme);
      assert.equal(await page.locator('#elevation-dialog').isVisible(), false);
      assert.equal(await page.evaluate(() => window.elevationCalls.some(c => c.command === 'restart_as_admin')), false);
      await page.locator('#nav-settings').click();
      const adminToggle = page.getByRole('switch', { name: lang === 'ru' ? 'Всегда запускать от имени администратора' : 'Always run as administrator' });
      await adminToggle.waitFor({ state: 'visible' });
      await page.waitForSelector('#always-run-as-admin-toggle:enabled');
      assert.equal(await adminToggle.getAttribute('aria-checked'), 'false');
      await adminToggle.focus();
      await page.keyboard.press('Space');
      await page.waitForSelector('#always-run-as-admin-toggle[aria-checked="true"]');
      assert.equal(await page.evaluate(() => window.elevationCalls.some(c => c.command === 'restart_as_admin')), false);
      await page.reload();
      await page.waitForFunction(() => !document.documentElement.classList.contains('app-loading'));
      await page.locator('#nav-settings').click();
      await page.waitForSelector('#always-run-as-admin-toggle[aria-checked="true"]');
      await page.evaluate(() => { window.adminPreferenceSaveFailure = true; });
      await adminToggle.click();
      await page.locator('#always-run-as-admin-error').waitFor({ state: 'visible' });
      assert.equal(await adminToggle.getAttribute('aria-checked'), 'true');
      await page.evaluate(() => { window.adminPreferenceSaveFailure = false; });
      await adminToggle.click();
      await page.waitForSelector('#always-run-as-admin-toggle[aria-checked="false"]');
      assert.equal(await page.locator('#always-run-as-admin-error').isVisible(), false);
      assert.equal(await page.evaluate(() => window.elevationCalls.some(c => c.command === 'restart_as_admin')), false);
      await page.locator('#app-sidebar nav a').first().click();
      await page.locator('#connect-btn').click();
      await page.locator('#elevation-dialog').waitFor({ state: 'visible' });
      assert.match(await page.locator('#elevation-reason').textContent(), lang === 'ru' ? /службой/ : /service/);
      assert.equal(await page.locator('#elevation-cancel').evaluate(el => el === document.activeElement), true);
      const bounds = await page.locator('#elevation-dialog').boundingBox();
      assert.ok(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= width && bounds.y + bounds.height <= height);
      await page.screenshot({ path: `src-tauri/target/elevation-qa/${theme}-${lang}.png` });
      await page.keyboard.press('Escape');
      assert.equal(await page.locator('#elevation-dialog').isVisible(), false);
      await page.locator('#connect-btn').click();
      await page.locator('#elevation-restart').click();
      await page.locator('#elevation-error').waitFor({ state: 'visible' });
      assert.match(await page.locator('#elevation-error').textContent(), lang === 'ru' ? /отменён/ : /cancelled/);
      assert.equal(await page.locator('#elevation-cancel').isEnabled(), true);
      await page.evaluate(() => { window.restartFailure = 'elevation_failed: startup failed'; });
      await page.locator('#elevation-restart').click();
      await page.waitForFunction(text => document.getElementById('elevation-error').textContent.includes(text), lang === 'ru' ? 'Не удалось' : 'Could not');
      await page.locator('#elevation-cancel').click();
      await page.evaluate(async () => {
        window.backgroundPermission = true;
        await (await import('/lib/core.js')).invoke('get_application_rules');
      });
      assert.equal(await page.locator('#elevation-notice').isVisible(), true);
      assert.equal(await page.locator('#elevation-dialog').isVisible(), false);
      await page.locator('#elevation-notice-restart').click();
      await page.locator('#elevation-dialog').waitFor({ state: 'visible' });
      await page.locator('#elevation-cancel').click();
      const noticeBounds = await page.locator('#elevation-notice').boundingBox();
      assert.ok(noticeBounds.x >= 0 && noticeBounds.x + noticeBounds.width <= width && noticeBounds.y + noticeBounds.height <= height);
      await page.screenshot({ path: `src-tauri/target/elevation-qa/notice-${theme}-${lang}.png` });
      await page.locator('#elevation-notice-close').focus();
      assert.equal(await page.locator('#elevation-notice-close').getAttribute('aria-label'), lang === 'ru' ? 'Закрыть' : 'Close');
      await page.keyboard.press('Enter');
      assert.equal(await page.locator('#elevation-notice').isVisible(), false);
      await page.evaluate(async () => {
        const { invoke } = await import('/lib/core.js');
        await invoke('get_application_rules');
        await window.__TAURI__.event.emit('admin-required-background', 'Zapret: admin_required:service');
      });
      assert.equal(await page.locator('#elevation-notice').isVisible(), false, 'Dismissal survives background polling and events');
      await page.evaluate(async () => (await import('/features/navigation.js')).showSection('traffic'));
      await page.locator('#traffic-toggle-btn').click();
      await page.locator('#elevation-dialog').waitFor({ state: 'visible' });
      assert.match(await page.locator('#elevation-reason').textContent(), lang === 'ru' ? /трафика/ : /Traffic/);
      await page.evaluate(() => { window.restartFailure = ''; });
      await page.locator('#elevation-restart').click();
      await page.waitForFunction(() => window.elevationCalls.filter(c => c.command === 'restart_as_admin').length === 3);
      assert.equal(await page.evaluate(() => window.elevationCalls.findLast(c => c.command === 'restart_as_admin').args.context.section), 'traffic');
      assert.deepEqual(errors, []);
      await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?resume=1`);
      await page.waitForFunction(() => !document.documentElement.classList.contains('app-loading'));
      await page.locator('#section-traffic').waitFor({ state: 'visible' });
      assert.equal(await page.locator('#elevation-dialog').isVisible(), false);
      if (theme === 'zapret') {
        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?slow-warp=1`);
        await page.waitForFunction(() => window.failWarpDetection && !document.documentElement.classList.contains('app-loading'));
        await page.locator('#section-home').waitFor({ state: 'visible' });
        assert.equal(await page.locator('#warp-card').isVisible(), false);
        await page.locator('#nav-warp-settings').click();
        await page.waitForSelector('#warp-page-status[data-state="detecting"]');
        assert.equal(await page.locator('#warp-sites-add').isDisabled(), true);
        await page.evaluate(() => window.failWarpDetection());
        await page.locator('#warp-page-error').waitFor({ state: 'visible' });
        assert.equal(await page.locator('#startup-retry').isVisible(), false);
        await page.evaluate(() => { window.warpRecovered = true; });
        await page.waitForSelector('#warp-sites-add:enabled');
        await page.locator('#warp-page-error').waitFor({ state: 'hidden' });

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?removed-warp=1`);
        await page.waitForFunction(() => !document.documentElement.classList.contains('app-loading'));
        await page.locator('#warp-card-initializing').waitFor({ state: 'visible' });
        assert.equal(await page.locator('#warp-card-controls').evaluate(el => el.inert), true);
        assert.equal(await page.evaluate(() => {
          const fit = window.elevationCalls.findIndex(call => call.command === 'fit_initial_window_height');
          const show = window.elevationCalls.findIndex(call => call.command === 'show_app_window');
          const content = document.getElementById('sections-host').getBoundingClientRect();
          const home = document.getElementById('section-home').getBoundingClientRect();
          return fit >= 0 && fit < show && window.elevationCalls[fit].args.height === Math.ceil(content.top + home.height);
        }), true);
        await page.evaluate(() => window.finishWarpDetection());
        await page.locator('#warp-card').waitFor({ state: 'hidden' });
        await page.reload();
        await page.waitForFunction(() => window.finishWarpDetection && !document.documentElement.classList.contains('app-loading'));
        assert.equal(await page.locator('#warp-card').isVisible(), false);

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?setup=1&repeat=true&clearLists=true`);
        await page.locator('#setup-dialog').waitFor({ state: 'visible' });
        assert.equal(await page.locator('#elevation-dialog').isVisible(), false);
        await page.locator('#setup-next').click();
        await page.locator('#setup-next').click();
        await page.locator('#elevation-dialog').waitFor({ state: 'visible' });
        assert.match(await page.locator('#elevation-reason').textContent(), /тестирования/);
        await page.locator('#elevation-restart').click();
        await page.locator('#elevation-error').waitFor({ state: 'visible' });
        assert.deepEqual(await page.evaluate(() => {
          const context = window.elevationCalls.findLast(c => c.command === 'restart_as_admin').args.context;
          return [context.section, context.repeat, context.clear_lists];
        }), ['setup', true, true]);
        await page.locator('#elevation-cancel').click();
        assert.equal(await page.locator('#setup-dialog').isVisible(), true);
      }
      await page.close();
    }
  } finally { await browser?.close(); await server.close(); }
});
