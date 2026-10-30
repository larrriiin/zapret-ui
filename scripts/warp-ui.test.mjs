// Isolated IPC fixture; never connects WARP or modifies the Windows proxy.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';

test('WARP settings workflows, traffic navigation, update modal and RU/EN themes render at desktop and minimum size', { timeout: 90000 }, async () => {
  const fixture = fileURLToPath(new URL('./fixtures/warp-ui.js', import.meta.url)).replaceAll('\\', '/');
  const shell = new URL('../src/index.html', import.meta.url);
  const server = await createServer({ server: { port: 0, strictPort: false }, plugins: [{
    name: 'warp-browser-tests', configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        if (!request.url?.startsWith('/warp-preview.html')) return next();
        try {
          const html = (await readFile(shell, 'utf8')).replace('src="/main.js"', `src="/@fs/${fixture}"`);
          response.setHeader('Content-Type', 'text/html'); response.end(await server.transformIndexHtml(request.url, html));
        } catch (error) { next(error); }
      });
    },
  }] });
  let browser;
  try {
    await server.listen();
    browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL || 'msedge', headless: true });
    const variants = [
      { width: 1100, height: 850, lang: 'ru', theme: 'graphite' },
      { width: 900, height: 750, lang: 'en', theme: 'light' },
    ];
    for (const variant of variants) {
      const context = await browser.newContext({ viewport: variant });
      const page = await context.newPage(); const errors = [];
      page.on('pageerror', error => errors.push(String(error)));
      await page.goto(`http://127.0.0.1:${server.config.server.port}/warp-preview.html?lang=${variant.lang}&theme=${variant.theme}`);
      await page.waitForSelector('#test-results', { timeout: 35000 });
      const results = await page.locator('#test-results').textContent();
      assert.equal(results.split('\n').filter(line => line.startsWith('FAIL')).join('\n'), '', results);
      assert.deepEqual(errors, []);


      assert.equal(await page.locator('vite-error-overlay').count(), 0);
      assert.ok(await page.title());
      await page.locator('#warp-advanced').evaluate(element => { element.open = true; });
      await page.locator('#warp-protocol-trigger').scrollIntoViewIfNeeded();
      assert.equal(await page.locator('#warp-protocol').inputValue(), 'MASQUE');
      assert.equal(await page.locator('#warp-auto-connect, #warp-check, #warp-page-connect').count(), 0);
      assert.equal(await page.locator('#warp-advanced #warp-auto-restore').count(), 1);
      assert.equal(await page.locator('#warp-advanced #warp-view-traffic').count(), 1);
      assert.equal(await page.locator('[data-warp-info="traffic"]').count(), 0);
      assert.deepEqual(await page.locator('#warp-mode option').evaluateAll(options => options.map(option => option.value)), ['warp', 'proxy', 'doh', 'dot', 'warp+dot', 'warp+doh', 'tunnel_only']);
      await page.locator('#warp-view-traffic').scrollIntoViewIfNeeded();
      const traffic = await page.locator('#warp-view-traffic').boundingBox();
      const diagnostics = await page.locator('#warp-diag-create').boundingBox();
      assert.ok(Math.abs(traffic.y - diagnostics.y) < 1, 'Traffic and diagnostics share one row');
      assert.ok(Math.abs(traffic.width - diagnostics.width) < 1 && traffic.height === diagnostics.height, 'Traffic and diagnostics have equal sizes');
      assert.equal(await page.locator('#warp-diag-create .material-symbols-outlined').textContent(), 'folder_zip');
      await page.locator('#warp-protocol-trigger').focus();
      await page.keyboard.press('ArrowDown');
      assert.equal(await page.locator('#warp-protocol-options').isVisible(), true);
      assert.deepEqual(await page.locator('#warp-protocol-options [role="option"]').allTextContents(), ['chevron_rightMASQUE', 'chevron_rightWireGuard']);
      assert.equal(await page.locator('#warp-protocol-options [aria-selected="true"]').evaluate(el => el === document.activeElement), true);
      if (process.env.QA_SCREENSHOTS_DIR) {
        await page.screenshot({ path: join(process.env.QA_SCREENSHOTS_DIR, `warp-protocol-${variant.lang}-${variant.width}.png`) });
      }
      await page.keyboard.press('ArrowDown');
      assert.equal(await page.locator('#warp-protocol-options [data-value="WireGuard"]').evaluate(el => el === document.activeElement), true);
      await page.keyboard.press('Enter');
      assert.equal(await page.locator('#confirm-modal').isVisible(), true);
      await page.locator('#confirm-modal-cancel').click();
      assert.equal(await page.locator('#warp-protocol').inputValue(), 'MASQUE');
      await page.locator('[data-warp-info="protocol"]').click();
      assert.match(await page.locator('#warp-settings-info-text').textContent(), /MASQUE/);
      await page.locator('#warp-settings-info-close').click();
      await page.locator('#warp-protocol-trigger').focus();
      await page.keyboard.press('ArrowDown'); await page.keyboard.press('Escape');
      assert.equal(await page.locator('#warp-protocol-trigger').evaluate(el => el === document.activeElement), true);
      assert.equal(await page.locator('#warp-protocol-options').isVisible(), false);

      const overflow = await page.locator('#section-warp-settings').evaluate(element => element.scrollWidth > element.clientWidth);
      assert.equal(overflow, false, 'WARP settings overflow the minimum window width');
      // Visible checks open the existing all-components modal, without an in-app toast.
      await page.evaluate(async () => {
        const original = window.__TAURI__.core.invoke;
        window.__TAURI__.core.invoke = async (command, args) => {
          if (command === 'get_ui_version_cmd') return '26.10.3';
          if (command === 'get_local_version_cmd') return '1.10';
          if (command === 'get_core_update_info') return { status: 'update_available', currentVersion: '1.10', stableVersion: '1.12' };
          if (command === 'get_telegram_status') return { installed: true, version: '1.10.3' };
          if (command === 'check_telegram_update') return { installed: true, current: '1.10.3', latest: '2.1', available: true };
          return original(command, args);
        };
        window.__TAURI__.updater = { check: async () => ({ version: '27.1' }) };
        const { checkForUpdates } = await import('/features/updates.js');
        await checkForUpdates(false);
      });
      assert.equal(await page.locator('#update-toast').count(), 0);
      assert.equal(await page.locator('#update-modal').isVisible(), true);
      assert.equal(await page.locator('#modal-update-ui-btn, #modal-update-core-btn, #modal-update-telegram-btn').count(), 3);
      assert.match(await page.locator('#update-modal').textContent(), /27\.1/);
      assert.match(await page.locator('#update-modal').textContent(), /1\.12/);
      assert.match(await page.locator('#update-modal').textContent(), /2\.1/);
      if (process.env.QA_SCREENSHOTS_DIR) {
        await page.screenshot({ path: join(process.env.QA_SCREENSHOTS_DIR, `updates-modal-${variant.lang}-${variant.width}.png`) });
      }
      await page.locator('#modal-close-btn').click();
      await page.evaluate(async () => { const { checkForUpdates } = await import('/features/updates.js'); await checkForUpdates(false); });
      assert.equal(await page.locator('#update-modal').count(), 0, 'Same background check must not repeatedly reopen the modal');
      await page.evaluate(async () => { const { checkForUpdates } = await import('/features/updates.js'); await checkForUpdates(true); });
      assert.equal(await page.locator('#update-modal').isVisible(), true, 'Manual check always opens detailed results');
      await page.locator('#modal-close-btn').click();
      if (process.env.QA_SCREENSHOTS_DIR) {
        await mkdir(process.env.QA_SCREENSHOTS_DIR, { recursive: true });
        await page.locator('#warp-page-title').scrollIntoViewIfNeeded();
        await page.screenshot({ path: join(process.env.QA_SCREENSHOTS_DIR, `warp-${variant.lang}-${variant.width}.png`) });
        await page.locator('#warp-diagnostics-title').scrollIntoViewIfNeeded();
        await page.screenshot({ path: join(process.env.QA_SCREENSHOTS_DIR, `warp-advanced-${variant.lang}-${variant.width}.png`) });
      }
      await page.locator('aside nav a').first().click();
      await page.waitForSelector('#section-home:not(.hidden)'); await page.waitForTimeout(200);
      for (const selector of ['#warp-title', '.zapret-connection .provider-heading .provider-title-row']) {
        const row = page.locator(selector);
        const metrics = await row.evaluate(e => { const text = e.querySelector(':scope > span').getBoundingClientRect(), button = e.querySelector('button').getBoundingClientRect(); return {text: text.y + text.height / 2, button: button.y + button.height / 2}; });
        assert.ok(Math.abs(metrics.text - metrics.button) <= 1, `${selector}: info icon aligns with title`);
      }
      await mkdir('artifacts/title-alignment', { recursive: true });
      await page.screenshot({ path: `artifacts/title-alignment/home-${variant.lang}-${variant.width}.png` });
      await page.locator('#warp-refresh').click();
      await page.waitForFunction(() => document.getElementById('warp-status-report').getAttribute('aria-busy') === 'false');
      assert.equal(await page.locator('#warp-status-dialog').isVisible(), true);
      assert.match(await page.locator('#warp-status-report').textContent(), /MASQUE/);
      assert.match(await page.locator('#warp-status-report').textContent(), /www.cloudflare.com/);
      assert.equal(await page.locator('#warp-status-dialog').evaluate(el => el.scrollWidth > el.clientWidth), false, 'Status dialog overflows horizontally');
      if (process.env.QA_SCREENSHOTS_DIR) {
        await page.screenshot({ path: join(process.env.QA_SCREENSHOTS_DIR, `warp-status-${variant.lang}-${variant.width}.png`) });
      }
      await page.locator('#warp-status-close').click();
      await context.close();
    }
  } finally { await browser?.close(); await server.close(); }
});
