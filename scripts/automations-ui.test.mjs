// Browser plugin not available. Playwright exercises isolated IPC fixtures; no system connections change.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';

test('application rules: CRUD, failure rollback, priority, keyboard dropdowns, themes and minimum window', { timeout: 90000 }, async () => {
  const fixture = fileURLToPath(new URL('./fixtures/automations-ui.js', import.meta.url)).replaceAll('\\', '/');
  const server = await createServer({ server: { port: 0, strictPort: false }, plugins: [{ name: 'automations-test', configureServer(server) {
    server.middlewares.use(async (req, res, next) => {
      if (!req.url?.startsWith('/automations-test.html')) return next();
      try {
        const html = (await readFile(new URL('../src/index.html', import.meta.url), 'utf8')).replace('src="/main.js"', `src="/@fs/${fixture}"`);
        res.setHeader('Content-Type', 'text/html'); res.end(await server.transformIndexHtml(req.url, html));
      } catch (e) { next(e); }
    });
  } }] });
  let browser;
  try {
    await server.listen(); browser = await chromium.launch({ channel: 'msedge', headless: true });
    await mkdir('artifacts/automations-qa', { recursive: true });
    for (const [theme, lang, width, height] of [['zapret','ru',1100,900], ['light','en',900,750], ['graphite','ru',900,750]]) {
      const page = await browser.newPage({ viewport: { width, height }, reducedMotion: 'reduce' }); const errors = [];
      page.on('pageerror', e => errors.push(String(e)));
      await page.goto(`http://127.0.0.1:${server.config.server.port}/automations-test.html?theme=${theme}&lang=${lang}`);
      await page.waitForSelector('#auto-empty:visible');
      assert.equal(await page.title(), 'ZAPRET UI');
      assert.equal(await page.locator('html').getAttribute('data-theme'), theme);
      assert.equal(await page.locator('vite-error-overlay').count(), 0);
      const headerBounds = await page.locator('#app-header').boundingBox(), railBounds = await page.locator('#app-sidebar').boundingBox();
      assert.equal(headerBounds.x, railBounds.x);
      assert.ok(railBounds.y >= headerBounds.y + headerBounds.height + 10);
      assert.equal(await page.locator('#app-header').evaluate(e => getComputedStyle(e).backgroundColor), 'rgba(0, 0, 0, 0)');
      assert.equal(await page.locator('#app-header').evaluate(e => getComputedStyle(e).backdropFilter), 'none');
      assert.equal(await page.locator('#sections-host').evaluate(e => getComputedStyle(e).backgroundColor), 'rgba(0, 0, 0, 0)');
      assert.equal(await page.locator('body').evaluate(e => getComputedStyle(e).isolation), 'isolate');

      for (const section of ['home', 'zapret-settings', 'traffic', 'settings', 'telegram', 'warp-settings', 'automations']) {
        await page.evaluate(async name => { const { showSection } = await import('/features/navigation.js'); showSection(name); document.querySelector('#sections-host').scrollTop = 0; }, section);
        await page.waitForSelector(`#section-${section}:not(.hidden)`); await page.waitForTimeout(200);
        const first = await page.locator(`#section-${section} > :first-child`).boundingBox();
        assert.ok(Math.abs(first.y - railBounds.y) <= 1, `${section}: first block at ${first.y}, rail at ${railBounds.y}`);
        assert.equal(await page.locator(`#section-${section}`).evaluate(e => getComputedStyle(e).backgroundColor), 'rgba(0, 0, 0, 0)');
        if (['home', 'settings', 'traffic'].includes(section)) await page.screenshot({ path: `artifacts/automations-qa/${theme}-${lang}-background-${section}.png` });

      }


      await page.evaluate(async () => { const { showSection } = await import('/features/navigation.js'); showSection('home'); });
      await page.waitForSelector('#section-home:not(.hidden)'); await page.waitForTimeout(200);
      await page.click('#ipset-info-btn'); await page.waitForSelector('#info-modal:not(.hidden)');
      const closeStyle = selector => page.locator(selector).evaluate(e => { const s = getComputedStyle(e); return [s.minHeight, s.padding, s.fontSize, s.borderRadius, s.backgroundColor]; });
      assert.deepEqual(await closeStyle('#info-modal-close'), await closeStyle('#auto-info-dialog form button'));
      assert.deepEqual(await closeStyle('#status-modal-done'), await closeStyle('#auto-info-dialog form button'));
      await page.screenshot({ path: `artifacts/automations-qa/${theme}-${lang}-home-info.png` });
      await page.click('#info-modal-close');
      await page.evaluate(async () => { const { showSection } = await import('/features/navigation.js'); showSection('automations'); });
      await page.waitForSelector('#section-automations:not(.hidden)'); await page.waitForTimeout(200);
      assert.equal(await page.locator('#app-header #titlebar').count(), 1);
      const windowControls = await page.locator('#titlebar').boundingBox();
      assert.ok(windowControls.x + windowControls.width <= headerBounds.x + headerBounds.width);
      assert.ok(Math.abs(windowControls.y + windowControls.height / 2 - headerBounds.y - headerBounds.height / 2) < 2);
      assert.equal(await page.locator('#tb-close').getAttribute('data-tauri-drag-region'), null);
      await page.click('#tb-minimize'); await page.click('#tb-maximize');
      await page.waitForFunction(() => document.querySelector('#tb-maximize-icon').textContent === 'filter_none');
      await page.click('#tb-maximize');
      await page.waitForFunction(() => document.querySelector('#tb-maximize-icon').textContent === 'crop_square');
      await page.click('#tb-close'); await page.waitForSelector('#close-confirm-modal:not(.hidden)');
      await page.click('#close-confirm-cancel');
      await page.click('#tb-close', { button: 'right' }); await page.waitForSelector('#tb-close-menu:not(.hidden)');
      await page.click('#tb-close-menu-tray');
      assert.deepEqual(await page.evaluate(() => window.testWindowCalls), ['minimize', 'maximize', 'maximize', 'close']);

      await page.locator('#nav-warp-settings, #nav-telegram').evaluateAll(nodes => nodes.forEach(node => { node.hidden = false; }));
      const trafficBounds = await page.locator('#nav-traffic').boundingBox(), projectBounds = await page.locator('#project-github-link').boundingBox();
      assert.ok(trafficBounds.y + trafficBounds.height <= projectBounds.y);
      await page.locator('#nav-warp-settings, #nav-telegram').evaluateAll(nodes => nodes.forEach(node => { node.hidden = true; }));

      assert.equal(await page.locator('[data-i18n="auto_desc"]').textContent().then(s => s.endsWith('.')), false);
      const titleBounds = await page.locator('#auto-title').boundingBox(), toggleBounds = await page.locator('#auto-enabled').boundingBox();
      assert.ok(toggleBounds.x >= titleBounds.x + titleBounds.width);
      assert.ok(Math.abs(toggleBounds.y + toggleBounds.height / 2 - titleBounds.y - titleBounds.height / 2) < 2);
      assert.equal(await page.locator('#section-automations label[data-i18n="auto_enabled"]').count(), 0);
      assert.equal(await page.locator('#auto-sort-hint').isVisible(), false);
      await page.click('#auto-info');
      assert.equal(await page.locator('#auto-info-dialog').evaluate(e => e.open), true);
      assert.equal(await page.locator('#auto-info-dialog [data-i18n="auto_constraints"]').isVisible(), true);
      await page.keyboard.press('Escape');
      assert.equal(await page.locator('#auto-info-dialog').evaluate(e => e.open), false);
      await page.click('#auto-info'); await page.click('#auto-info-close');
      assert.equal(await page.locator('#auto-info-dialog').evaluate(e => e.open), false);

      const pick = async (name, value) => { const trigger = page.locator(`#auto-${name}-trigger`); await trigger.evaluate(e => e.scrollIntoView({ block: 'center' })); await page.waitForTimeout(150); await trigger.click(); await page.locator(`#auto-${name}-options-list [data-value="${value}"]`).click(); };
      await page.click('#auto-add');
      assert.equal(await page.locator('#auto-editor').evaluate(e => e.tagName), 'DIALOG');
      assert.equal(await page.locator('#auto-editor summary').count(), 0);
      await page.fill('#auto-name', 'Unsaved draft'); await page.keyboard.press('Escape');
      await page.waitForFunction(() => !document.querySelector('#auto-editor').open);
      assert.equal(await page.locator('#auto-add').evaluate(e => document.activeElement === e), true);
      await page.click('#auto-add'); assert.equal(await page.inputValue('#auto-name'), '');
      await page.fill('#auto-name', 'Another draft'); await page.mouse.click(4, height - 4);
      await page.waitForFunction(() => !document.querySelector('#auto-editor').open);
      assert.equal(await page.locator('#auto-list article').count(), 0);
      await page.click('#auto-add'); assert.equal(await page.inputValue('#auto-name'), '');
      await page.click('#auto-choose');
      assert.equal(await page.inputValue('#auto-name'), 'game');
      await page.locator('#auto-icons [data-icon="sports_esports"]').click();
      await pick('warp', 'on');
      await pick('mode', 'proxy');
      await page.locator('[name="auto-route-source"][value="custom"]').check();
      await page.locator('#auto-proxy-sites input').first().check();
      await page.locator('#auto-proxy-applications input').first().check();
      await page.locator('#auto-proxy-picker').scrollIntoViewIfNeeded();
      await page.screenshot({path: `artifacts/automations-qa/${theme}-${lang}-proxy-picker.png`});
      await pick('mode', 'warp'); assert.equal(await page.locator('#auto-proxy-picker').isVisible(), false);
      await pick('mode', 'proxy'); assert.equal(await page.locator('#auto-proxy-sites input').first().isChecked(), true);

      await page.click('#auto-save'); await page.waitForSelector('#auto-list article');
      assert.equal(await page.evaluate(() => window.testConfig.rules[0].warp_mode), 'proxy');
      assert.equal(await page.locator('#auto-list [data-provider="warp"]').count(), 1);
      assert.equal(await page.locator('#auto-list [data-provider="zapret"]').count(), 0);
      assert.equal(await page.locator('#auto-list .warp-cloud').textContent(), 'cloud');
      assert.ok((await page.locator('#auto-list .auto-rule-provider-value').textContent()).includes(lang === 'ru' ? 'Локальный прокси' : 'Local proxy'));
      assert.equal(await page.locator('#auto-list [data-rule-state]').isVisible(), false);
      const normalBorder = await page.locator('#auto-list article').first().evaluate(e => getComputedStyle(e).borderColor);
      await page.evaluate(() => { window.testActive = [window.testConfig.rules[0].id]; });
      await page.waitForSelector('#auto-list .ui-panel-active');
      assert.notEqual(await page.locator('#auto-list article').first().evaluate(e => getComputedStyle(e).borderColor), normalBorder);
      assert.equal(await page.locator('#auto-list [data-rule-state]').getAttribute('class'), 'sr-only');
      await page.screenshot({path:`artifacts/automations-qa/${theme}-${lang}-active-border.png`});
      await page.evaluate(() => { window.testActive = []; });
      await page.waitForFunction(() => !document.querySelector('#auto-list .ui-panel-active'));
      assert.equal(await page.locator('#auto-list article').first().evaluate(e => getComputedStyle(e).borderColor), normalBorder);


      assert.deepEqual(await page.evaluate(() => window.testConfig.rules[0].warp_routes), { sites: ['example.org'], applications: ['C:\\Apps\\Discord.exe'] });

      assert.equal(await page.evaluate(() => window.testConfig.rules[0].icon), 'sports_esports');
      await page.locator('#auto-list .auto-more').first().click(); await page.click('[data-action="auto_edit"]');
      assert.equal(await page.locator('#auto-proxy-sites input').first().isChecked(), true);
      assert.equal(await page.locator('#auto-proxy-applications input').first().isChecked(), true);
      await page.fill('#auto-name', '<img src=x onerror=alert(1)>');
      await page.evaluate(() => { window.testFailSave = true; });
      await page.click('#auto-save'); await page.waitForSelector('#auto-editor-error:visible');
      assert.equal(await page.evaluate(() => window.testConfig.rules[0].name), 'game');
      await page.evaluate(() => { window.testFailSave = false; });
      await page.click('#auto-save'); await page.waitForSelector('#auto-editor:not([open])', { state: 'attached' });
      assert.equal(await page.locator('#auto-list img').count(), 0);
      await page.locator('#auto-list .auto-more').first().click(); await page.click('[data-action="auto_edit"]');
      assert.equal(await page.locator('#auto-icons [data-icon="sports_esports"]').getAttribute('aria-pressed'), 'true');
      await page.fill('#auto-name', 'Game'); await page.click('#auto-save');
      await page.click('#auto-add'); await page.click('#auto-choose'); await page.fill('#auto-name', 'Discord');
      await page.locator('#auto-icons [data-icon="forum"]').click();
      await pick('zapret', 'on');
      await pick('strategy', 'general (ALT12)');
      await page.click('#auto-save'); await page.waitForFunction(() => window.testConfig.rules.length === 2);
      const first = page.locator('#auto-list article').first(), second = page.locator('#auto-list article').nth(1);
      await page.locator('#sections-host').evaluate(e => { e.scrollTop = e.scrollHeight; });
      const handle = await second.locator('.auto-drag-handle').boundingBox(), target = await first.boundingBox();
      // Escape cancels a preview and restores the stored priority.
      await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2); await page.mouse.down();
      await page.mouse.move(target.x + 28, target.y + 12, { steps: 12 }); await page.waitForTimeout(100);
      await page.keyboard.press('Escape'); await page.mouse.up();
      assert.equal(await page.locator('#auto-list article').first().locator('h3').textContent(), 'Game');
      assert.equal(await page.evaluate(() => window.testConfig.rules[0].name), 'Game');
      await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2); await page.mouse.down();
      await page.mouse.move(target.x + 28, target.y + 12, { steps: 12 }); await page.waitForTimeout(100); await page.mouse.up();
      await page.waitForFunction(() => window.testConfig.rules[0].name === 'Discord', null, {timeout:5000});
      await page.locator('#auto-list article').first().getByRole('switch').click();
      await page.waitForFunction(() => !window.testConfig.rules[0].enabled);
      await page.click('#auto-enabled'); await page.waitForFunction(() => !window.testConfig.enabled);
      await page.locator('#auto-list .auto-more').first().click(); await page.click('[data-action="auto_edit"]');
      await page.locator('#auto-close-trigger').scrollIntoViewIfNeeded(); await page.waitForTimeout(150); await page.focus('#auto-close-trigger'); await page.keyboard.press('ArrowDown'); await page.keyboard.press('End'); await page.keyboard.press('Enter');
      assert.equal(await page.inputValue('#auto-close'), 'off');
      await page.screenshot({ path: `artifacts/automations-qa/${theme}-${lang}-editor.png`, fullPage: true });
      assert.equal(await page.locator('#section-automations').evaluate(e => e.scrollWidth > e.clientWidth), false);
      assert.deepEqual(errors, []);
      await page.click('#auto-cancel');
      await page.locator('#sections-host').evaluate(e => { e.scrollTop = 0; });
      await page.screenshot({ path: `artifacts/automations-qa/${theme}-${lang}.png`, fullPage: true });
      // Menu keyboard dismissal returns focus; keyboard reorder persists and rolls back on failure.
      const more = page.locator('#auto-list .auto-more').first();
      await more.click();
      await page.screenshot({path: `artifacts/automations-qa/${theme}-${lang}-menu.png`, fullPage:true});
      await page.keyboard.press('End');
      assert.equal(await page.locator(':focus').getAttribute('data-action'), 'auto_remove');
      await page.keyboard.press('Escape'); assert.equal(await more.getAttribute('aria-expanded'), 'false');
      await page.locator('#auto-list .auto-drag-handle').first().focus(); await page.keyboard.press('ArrowDown');
      await page.waitForFunction(() => window.testConfig.rules[1].name === 'Discord');
      await page.evaluate(() => { window.testFailSave = true; });
      await page.locator('#auto-list .auto-drag-handle').last().focus(); await page.keyboard.press('ArrowUp');
      await page.waitForSelector('#auto-error:visible');
      assert.equal(await page.locator('#auto-list article').last().locator('h3').textContent(), 'Discord');
      await page.evaluate(() => { window.testFailSave = false; });
      await page.locator('#auto-list .auto-more').first().click(); await page.click('[data-action="auto_remove"]');
      await page.click('#confirm-modal-ok'); await page.waitForFunction(() => window.testConfig.rules.length === 1);
      await page.close();
    }
  } finally { await browser?.close(); await server.close(); }
});
