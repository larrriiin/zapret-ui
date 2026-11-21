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
    await server.listen(); browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge', headless: true });
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
          return fit >= 0 && fit < show && window.elevationCalls[fit].args.height === Math.ceil(content.top + home.height + 24);
        }), true);
        await page.evaluate(() => window.finishWarpDetection());
        await page.locator('#warp-card').waitFor({ state: 'hidden' });
        await page.reload();
        await page.waitForFunction(() => window.finishWarpDetection && !document.documentElement.classList.contains('app-loading'));
        assert.equal(await page.locator('#warp-card').isVisible(), false);

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?wiresock-startup=1`);
        await page.waitForFunction(() => window.finishWireSockDetection && !document.documentElement.classList.contains('app-loading'));
        assert.equal(await page.locator('#aux-provider-switch').isVisible(), true);
        assert.equal(await page.evaluate(() => {
          const rect = document.getElementById('section-home').getBoundingClientRect();
          return window.elevationCalls.find(call => call.command === 'fit_initial_window_height').args.height === Math.ceil(rect.bottom + 24);
        }), true);
        await page.evaluate(() => window.finishWireSockDetection());

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?wiresock-permission=1`);
        await page.locator('#aux-provider-wiresock').click();
        await page.locator('#wiresock-connect').click();
        await page.locator('#wiresock-setup').click();
        await page.locator('#elevation-dialog').waitFor();
        await page.locator('#elevation-restart').click();
        await page.locator('#elevation-error').waitFor();
        const wireSockHandoff = await page.evaluate(() => window.elevationCalls.findLast(c => c.command === 'restart_as_admin').args.context.wiresock_setup);
        assert.deepEqual(wireSockHandoff, { profile: 'Office VPN', generated: false, strategy: 'general' });
        await page.locator('#elevation-cancel').click();
        assert.equal(await page.locator('#wiresock-setup-dialog').isVisible(), true);
        assert.equal(await page.evaluate(() => window.elevationCalls.some(c => c.command === 'connect_wiresock')), false);

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?wiresock-resume=1`);
        await page.waitForSelector('#wiresock-connect[data-action="stop"]');
        assert.equal(await page.evaluate(() => localStorage.getItem('zapret.wiresock.lastConnected')), 'Office VPN');
        assert.equal(await page.evaluate(() => window.elevationCalls.filter(c => c.command === 'resume_wiresock_setup').length), 1);

        await page.goto(`http://127.0.0.1:${server.config.server.port}/elevation-test.html?setup=1&wiresock-onboarding=1`);
        const currentStep = page.locator('.setup-steps [aria-current="step"]');
        assert.equal(await currentStep.getAttribute('data-setup-step'), '0');
        await page.emulateMedia({ reducedMotion: 'no-preference' });
        const animation = await page.evaluate(async () => {
          const rail = document.querySelector('.setup-step-fill');
          const marker = document.querySelector('[data-setup-step="1"] .setup-step-marker');
          const check = document.querySelector('[data-setup-step="0"] path');
          const samples = [];
          const sample = () => ({
            rail: Number(rail.style.getPropertyValue('--step-progress')),
            marker: Number(marker.style.transform.match(/[\d.]+/)[0]),
            check: Number(check.style.strokeDashoffset),
          });
          document.getElementById('setup-next').click();
          samples.push(sample());
          const start = performance.now();
          while (performance.now() - start < 550) {
            await new Promise(requestAnimationFrame);
            samples.push(sample());
          }
          return samples;
        });
        assert.deepEqual(animation[0], { rail: 0, marker: 0.7, check: 22 });
        assert.ok(animation.some(sample => sample.rail > 0 && sample.rail < 0.3 && sample.check === 22));
        assert.ok(animation.some(sample => sample.marker > 1.1));
        assert.ok(animation.at(-1).rail > 0.32 && animation.at(-1).check < 7);
        assert.equal(await currentStep.getAttribute('data-setup-step'), '1');
        assert.equal(await page.locator('.setup-steps [data-setup-step="0"] [data-setup-complete]').getAttribute('hidden'), null);
        await page.locator('#setup-back').click();
        await page.waitForFunction(() => Number(document.querySelector('.setup-step-fill').style.getPropertyValue('--step-progress')) < 0.02);
        assert.equal(await currentStep.getAttribute('data-setup-step'), '0');
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.locator('#setup-next').click();
        await page.setViewportSize({ width: 480, height: 460 });
        assert.equal(await page.locator('#setup-dialog').evaluate(dialog => dialog.scrollWidth > innerWidth), false);
        assert.equal(await page.locator('#setup-body').evaluate(body => body.scrollHeight > body.clientHeight), true);
        const footer = await page.locator('.setup-footer').first().boundingBox();
        assert.ok(footer.y + footer.height <= 460);
        await page.locator('#setup-wiresock').check();
        await page.locator('#setup-next').click();
        await page.locator('[data-setup-panel="2"]').waitFor();
        assert.equal(await currentStep.getAttribute('data-setup-step'), '2');
        assert.equal(await page.evaluate(() => window.elevationCalls.filter(c => c.command === 'install_wiresock').length), 1);
        assert.match(await page.locator('#setup-wiresock-detail').textContent(), /3\.8\.2\.1/);
        await page.locator('#setup-skip').click();
        assert.equal(await currentStep.getAttribute('data-setup-step'), '3');
        assert.equal(await page.locator('.setup-steps [data-complete]').count(), 4);
        assert.equal(await page.locator('.setup-step-fill').evaluate(bar => Number(bar.style.getPropertyValue('--step-progress'))), 1);
        await page.setViewportSize({ width, height });

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
