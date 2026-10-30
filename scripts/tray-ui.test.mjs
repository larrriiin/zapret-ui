// Browser plugin not available; Playwright validates menu UI with isolated Tauri IPC.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';
test('tray: saved theme/accent, locales, keyboard, strategy selection, actions and dismissal', { timeout: 60000 }, async () => {
  const server = await createServer({ server: { port: 0, strictPort: false } });
  let browser;
  try {
    await server.listen();
    browser = await chromium.launch({ channel: 'msedge', headless: true });
    await mkdir('artifacts/tray-qa', { recursive: true });
    for (const [theme, lang] of [['zapret', 'ru'], ['graphite', 'ru'], ['light', 'en']]) {
      const page = await browser.newPage({ viewport: { width: 272, height: 264 }, reducedMotion: 'reduce' });
      const errors = []; page.on('pageerror', e => errors.push(String(e)));
      await page.addInitScript(({ theme, lang }) => {
        localStorage.setItem('zapret_theme', theme); localStorage.setItem('zapret_lang', lang);
        localStorage.setItem('zapret_accents', JSON.stringify({ zapret: 'mint', graphite: 'sage', light: 'teal' }));
        window.calls = []; window.handlers = {};
        window.__TAURI__ = {
          event: { listen: async (name, cb) => { window.handlers[name] = cb; return () => {}; } },
          window: { getCurrentWindow: () => ({ show: async () => window.calls.push('window-show'), hide: async () => window.calls.push('hide'), setFocus: async () => {} }) },
          core: { invoke: async (cmd, args) => {
            if (cmd === 'tray_action') {
              window.calls.push(args.action);
              if (window.delayNextAction) {
                window.delayNextAction = false;
                return new Promise((resolve, reject) => { window.failAction = () => reject('Service stop failed'); });
              }
              return;
            }
            if (cmd !== 'tray_snapshot') return;
            return { strategy_name: 'general', zapret_active: theme !== 'light', warp_active: theme === 'light', status: { text: lang === 'ru' ? 'Статус: Подключено' : 'Status: Connected' }, strategy: { text: 'Strategy: general' },
              toggle: { text: lang === 'ru' ? 'Выключить Zapret' : 'Turn Off Zapret', enabled: true },
              warp_status: { text: 'WARP: Connected' }, warp_toggle: { text: 'Disconnect WARP', enabled: false },
              strategies: ['general', 'general (ALT)', '<unsafe>'] };
          } }
        };
      }, { theme, lang });
      await page.goto(`http://127.0.0.1:${server.config.server.port}/tray.html`);
      await page.waitForFunction(() => window.handlers['tray-opening']);
      await page.evaluate(() => window.handlers['tray-opening']());
      await page.waitForFunction(() => window.calls.includes('window-show'));
      assert.equal(await page.title(), 'ZAPRET UI');
      assert.equal(await page.locator('html').getAttribute('data-theme'), theme);
      assert.equal(await page.locator('html').getAttribute('data-accent'), { zapret: 'mint', graphite: 'sage', light: 'teal' }[theme]);
      assert.equal(await page.getByRole('menuitem').count(), 6);
      assert.equal(await page.locator('button:focus').count(), 0);
      assert.equal(await page.locator('[role=menu]').evaluate(el => getComputedStyle(el).outlineStyle), 'none');
      assert.equal(await page.locator('.tray-summary').count(), 0);
      assert.equal(await page.locator('[data-action="strategy"] .tray-strategy-name').innerText(), 'general');
      assert.equal(await page.locator('[data-action="toggle"] .ui-status-dot').count(), 1);
      assert.equal(await page.locator('[data-action="warp_toggle"] .ui-status-dot').count(), 1);
      assert.equal(await page.locator('button:disabled').count(), 1);
      assert.equal(await page.locator('vite-error-overlay').count(), 0);
      assert.equal(await page.locator('.ui-status-dot[data-active="true"]').count(), 1);
      assert.equal(await page.locator('.ui-status-dot[data-active="false"]').count(), 1);
      assert.equal(await page.locator('.tray-menu').evaluate(el => getComputedStyle(el).borderRadius), '0px');
      assert.equal(await page.locator('.tray-menu').evaluate(el => getComputedStyle(el).borderTopWidth), '0px');
      const last = await page.getByRole('menuitem').last().boundingBox();
      assert.ok(last.y + last.height <= 264, 'all actions fit the compact window');
      await page.screenshot({ path: `artifacts/tray-qa/${theme}.png` });
      await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown'); await page.keyboard.press('Enter');
      await page.getByRole('menuitem', { name: '<unsafe>', exact: true }).waitFor();
      assert.equal(await page.locator('unsafe').count(), 0);
      await page.getByRole('menuitem', { name: '<unsafe>', exact: true }).click();
      assert.ok(await page.evaluate(() => window.calls.includes('strat_<unsafe>')));
      await page.evaluate(() => window.handlers['tray-opening']());
      await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown'); await page.keyboard.press('Enter');
      await page.getByRole('menuitem', { name: lang === 'ru' ? 'Назад' : 'Back', exact: true }).click();
      await page.getByRole('menuitem', { name: lang === 'ru' ? 'Развернуть' : 'Restore window', exact: true }).click();
      assert.ok(await page.evaluate(() => window.calls.includes('show')));
      await page.evaluate(() => window.handlers['tray-opening']());
      await page.keyboard.press('End');
      assert.equal(await page.locator('button:focus').innerText(), lang === 'ru' ? 'logout\nВыход' : 'logout\nExit');
      await page.keyboard.press('Escape'); assert.ok(await page.evaluate(() => window.calls.includes('hide')));
      // A slow service/UAC operation must stay busy across status refreshes.
      await page.evaluate(() => window.handlers['tray-opening']());
      await page.evaluate(() => { window.delayNextAction = true; });
      await page.getByRole('menuitem').first().click();
      await page.waitForFunction(() => Boolean(window.failAction));
      await page.waitForTimeout(1700);
      assert.equal(await page.locator('button:disabled').count(), 6);
      const prior = await page.evaluate(() => window.calls.length);
      await page.locator('button').first().evaluate(el => el.dispatchEvent(new MouseEvent('click')));
      assert.equal(await page.evaluate(() => window.calls.length), prior, 'duplicate click is ignored');
      await page.evaluate(() => window.failAction());
      await page.locator('#tray-error:visible').waitFor();
      assert.equal(await page.locator('#tray-error').innerText(), 'Service stop failed');
      assert.equal(await page.locator('button:disabled').count(), 1, 'controls recover after failure');
      assert.deepEqual(errors, []);
      await page.close();
    }
  } finally { await browser?.close(); await server.close(); }
});
