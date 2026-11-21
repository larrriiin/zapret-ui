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
      await context.addInitScript(() => localStorage.setItem('zapret.wiresock.lastConnected', 'Home VPN'));
      const page = await context.newPage(); const errors = [];
      page.on('pageerror', error => errors.push(String(error)));
      await page.goto(`http://127.0.0.1:${server.config.server.port}/warp-preview.html?lang=${variant.lang}&theme=${variant.theme}`);
      await page.waitForSelector('#test-results', { timeout: 35000 });
      const results = await page.locator('#test-results').textContent();
      assert.equal(results.split('\n').filter(line => line.startsWith('FAIL')).join('\n'), '', results);
      assert.deepEqual(errors, []);
      assert.ok((await page.evaluate(() => window.wireSockLabelsAtStartup)).every(label => label.trim()), 'WireSock dialog text exists before a language change');


      assert.equal(await page.locator('vite-error-overlay').count(), 0);
      assert.ok(await page.title());
      await page.evaluate(() => {
        const original = window.__TAURI__.core.invoke;
        let profiles = 0;
        window.profileRevealed = false;
        window.__TAURI__.core.invoke = async (command, args) => {
          if (command === 'reveal_warp_profile') {
            window.profileRevealed = true;
            return;
          }
          if (command === 'choose_warp_applications') return ['C:\\Games\\Steam\\steam.exe'];
          if (command !== 'save_warp_profile') return original(command, args);
          const defaults = { endpoint: '', randomEndpoint: true, maskingDomain: '',
            services: null, dns: 'cloudflare', excludeLan: false, ipv6: true, mtu: 1280, keepalive: 0 };
          const expected = profiles === 0 ? defaults : {
            ...defaults, endpoint: 'vpn.example:2408', randomEndpoint: false, maskingDomain: 'example.com',
            services: ['youtube', 'telegram'], excludeLan: true, ipv6: false, mtu: 1200, keepalive: 25,
            applications: { allowed: ['C:\\Games\\League of Legends\\LeagueClientUx.exe'], disallowed: ['C:\\Games\\Steam\\steam.exe'] },
            sites: ['telegram.org'],
          };
          if (!args.acceptedTerms || JSON.stringify(args.options) !== JSON.stringify(expected) || 'saveTitle' in args) {
            throw 'warp_profile_routes_error';
          }
          return `WARP-WireSock-${++profiles}.conf`;
        };
      });
      const profileOpen = page.locator('#warp-profile-open');
      const profileDialog = page.locator('#warp-profile-dialog');
      const profileSave = page.locator('#warp-profile-save');
      await profileOpen.click();
      assert.equal(await profileDialog.isVisible(), true);
      assert.equal(await profileDialog.evaluate(element => element.scrollWidth > element.clientWidth), false);
      assert.equal(await profileSave.isDisabled(), true);
      assert.equal(await page.locator('#warp-profile-endpoint').isVisible(), false);
      await page.locator('#warp-profile-terms').check();
      await profileSave.click();
      await page.locator('#warp-profile-status').filter({ hasText: 'WARP-WireSock-1.conf' }).waitFor();
      assert.equal(await page.locator('#warp-profile-hint').isVisible(), false);
      assert.equal(await page.locator('#warp-profile-options').isVisible(), false);
      assert.deepEqual(await profileDialog.locator('button:visible').evaluateAll(buttons => buttons.map(button => button.id)), ['warp-profile-reveal', 'warp-profile-close']);
      assert.equal(await page.locator('#warp-profile-close').evaluate(button => button === document.activeElement), true);
      await page.locator('#warp-profile-reveal').click();
      assert.equal(await page.evaluate(() => window.profileRevealed), true);
      await page.locator('#warp-profile-close').click();
      await profileOpen.click();
      assert.equal(await page.locator('#warp-profile-status').isVisible(), false);
      assert.equal(await profileSave.isVisible(), true);
      assert.equal(await profileSave.isDisabled(), true);
      await page.locator('#warp-profile-terms').check();
      assert.equal(await profileSave.isDisabled(), false);
      await page.locator('#warp-profile-advanced summary').press('Enter');
      assert.equal(await page.locator('#warp-profile-endpoint').isDisabled(), true);
      await page.locator('#warp-profile-random-endpoint').uncheck();
      assert.equal(await page.locator('#warp-profile-endpoint').isDisabled(), false);
      await page.locator('#warp-profile-endpoint').fill('vpn.example:2408');
      assert.equal(await page.locator('#warp-profile-masking-domain').isVisible(), true);
      await page.locator('#warp-profile-masking-domain').fill('example.com');
      await page.locator('#warp-profile-split').check();
      assert.equal(await profileSave.isDisabled(), true);
      assert.equal(await page.locator('#warp-profile-dns input[value="google"]').isDisabled(), true);
      await page.locator('#warp-profile-services input[value="youtube"]').check();
      await page.locator('#warp-profile-services input[value="telegram"]').check();
      const group = (dialog, id) => dialog.locator(`[data-rules-group="${id}"]`);
      await group(profileDialog, 'allowed').locator('[data-rules-copy]').click();
      await group(profileDialog, 'allowed').locator('.warp-rule-name').filter({ hasText: 'LeagueClientUx.exe' }).waitFor();
      await group(profileDialog, 'disallowed').locator('[data-rules-add]').click();
      await group(profileDialog, 'disallowed').locator('.warp-rule-name').filter({ hasText: 'steam.exe' }).waitFor();
      await group(profileDialog, 'sites').locator('[data-rules-add]').click();
      await page.locator('#warp-sites-input').fill('https://telegram.org/path');
      await page.locator('#warp-sites-dialog-add').click();
      await group(profileDialog, 'sites').locator('.warp-rule-name').filter({ hasText: 'telegram.org' }).waitFor();
      assert.equal(await page.locator('#warp-profile-apps-ip-hint').isVisible(), true);
      await page.locator('#warp-profile-exclude-lan').check();
      await page.locator('#warp-profile-ipv6').uncheck();
      await page.locator('#warp-profile-mtu').fill('1200');
      await page.locator('#warp-profile-keepalive').fill('25');
      await profileSave.click();
      await page.locator('#warp-profile-status').filter({ hasText: 'WARP-WireSock-2.conf' }).waitFor();
      await page.keyboard.press('Escape');
      assert.equal(await profileDialog.isVisible(), false);
      assert.equal(await profileOpen.evaluate(element => element === document.activeElement), true);

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
        const metrics = await row.evaluate(e => { const text = e.querySelector(':scope > span').getBoundingClientRect(), button = e.querySelector('.ui-icon-button').getBoundingClientRect(); return {text: text.y + text.height / 2, button: button.y + button.height / 2}; });
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

      assert.equal(await page.locator('#aux-provider-switch').isVisible(), false);
      await page.evaluate(() => {
        const original = window.__TAURI__.core.invoke;
        window.wire = { installed: false, state: 'disconnected', active: null, applied: 0, checks: 0, restarts: 0, restartPending: false, connections: 0, reconnections: 0,
          global: { enabled: false, applications: { allowed: [], disallowed: [] }, sites: [] }, splitTunneling: true, profiles: [
          { name: 'WARP-WireSock-1234567890ABCDEF', file_source: 'legacy_downloads', imported: false, created_at: 1784563200 },
          { name: 'Home VPN', file_source: null, imported: true, created_at: 1784563200 },
          ...Array.from({ length: 24 }, (_, index) => ({ name: `Saved VPN ${index + 1}`, file_source: null, imported: true, created_at: 1784563200 })),
        ] };
        const installation = () => ({ installed: window.wire.installed, version: window.wire.installed ? '3.8.2.1' : null });
        const status = () => ({ ...installation(), state: window.wire.state, profile: window.wire.active,
          address: window.wire.active ? '203.0.113.8' : null, split_tunneling: window.wire.splitTunneling });
        const connect = ({ profile: name, generated }) => {
          const profile = window.wire.profiles.find(item => item.name === name);
          if (!profile || generated !== (Boolean(profile.file_source) && !profile.imported)) throw 'wiresock_profile_missing';
          profile.imported = true;
          window.wire.state = 'connected'; window.wire.active = name;
          window.wire.connectedWithSplit = window.wire.splitTunneling;
          return status();
        };
        window.__TAURI__.core.invoke = async (command, args) => {
          const wire = window.wire;
          if (command === 'get_wiresock_status') return status();
          if (command === 'get_wiresock_installation') return installation();
          if (command === 'install_wiresock') {
            if (!wire.installTried) {
              wire.installTried = true;
              throw { code: 'wiresock_install_error', detail: 'WireSock Setup 1603\nC:\\Logs\\wiresock-install-test.log' };
            }
            wire.installed = true;
            return;
          }
          if (command === 'list_wiresock_profiles') {
            if (wire.failList) throw 'wiresock_cli_failed';
            return wire.profiles;
          }
          if (command === 'get_wiresock_global_rules') return structuredClone(wire.global);
          if (command === 'set_wiresock_split_tunneling') { wire.splitTunneling = args.enabled; return; }
          if (command === 'save_wiresock_global_rules') {
            wire.global = structuredClone(args.rules);
            return;
          }
          if (command === 'get_wiresock_split_rules') {
            if (args.profile !== 'Home VPN') throw 'wiresock_profile_missing';
            return { applications: { allowed: ['chrome.exe'], disallowed: [] }, sites: [] };
          }
          if (command === 'save_wiresock_split_profile') {
            if (args.profile !== 'Home VPN' || args.applications.allowed.length || args.applications.disallowed.join() !== 'C:\\Games\\Steam\\steam.exe' || args.sites.join() !== 'example.org') {
              throw 'wiresock_split_write_error';
            }
            const name = 'WireSock-Split-5555555555555555';
            wire.profiles.push({ name, file_source: 'app_data', imported: false, created_at: 1784563200 });
            return name;
          }
          if (command === 'get_wiresock_setup') {
            wire.checks++;
            return { domains: wire.applied ? 0 : 11, hosts: wire.applied ? 0 : 4, ipset: wire.applied ? 0 : 17, ipset_mode: 'loaded', restart_pending: wire.restartPending };
          }
          if (command === 'apply_wiresock_setup') {
            if (!args.confirmed) throw 'wiresock_setup_confirmation';
            wire.applied = 1;
            wire.restarts++;
            if (wire.restarts === 1) {
              wire.restartPending = true;
              throw 'wiresock_zapret_restart_failed';
            }
            wire.restartPending = false;
            return { status: connect(args.request) };
          }
          if (command === 'connect_wiresock') {
            wire.connections++;
            return connect(args);
          }
          if (command === 'reconnect_wiresock') {
            if (wire.state !== 'connected') throw 'wiresock_connect_failed';
            wire.reconnections++;
            return connect(args);
          }
          if (command === 'disconnect_wiresock') { wire.state = 'disconnected'; wire.active = null; return status(); }
          if (command === 'import_wiresock_profile') {
            wire.profiles.push({ name: 'Imported profile', file_source: null, imported: true, created_at: 1784563200 });
            return 'Imported profile';
          }
          if (command === 'delete_wiresock_profile') {
            if (wire.profiles.find(item => item.name === args.profile)?.file_source !== args.fileSource) throw 'wiresock_profile_missing';
            wire.profiles = wire.profiles.filter(item => item.name !== args.profile);
            return;
          }
          if (command === 'save_warp_profile') {
            const name = 'WARP-WireSock-5555555555555555';
            wire.profiles.unshift({ name, file_source: 'app_data', imported: false, created_at: 1784563200 });
            return `${name}.conf`;
          }
          return original(command, args);
        };
      });
      await page.locator('#nav-settings').click();
      assert.equal(await page.locator('#wiresock-settings-remove').isVisible(), false);
      await page.locator('#wiresock-install').click();
      await page.locator('#wiresock-install-result').filter({ hasText: 'WireSock Setup 1603' }).waitFor();
      assert.match(await page.locator('#wiresock-install-result').textContent(), /wiresock-install-test\.log/);
      await page.locator('#wiresock-install').click();
      await page.locator('#wiresock-settings-status').filter({ hasText: '3.8.2.1' }).waitFor();
      assert.equal(await page.locator('#wiresock-install').isVisible(), false);
      await page.locator('#wiresock-settings-refresh').click();
      await page.waitForFunction(() => document.getElementById('wiresock-status-report').getAttribute('aria-busy') === 'false');
      assert.match(await page.locator('#wiresock-status-report').textContent(), /3\.8\.2\.1/);
      await page.locator('#wiresock-status-close').click();
      await page.locator('#wiresock-settings-remove').click();
      await page.locator('#wiresock-remove-dialog').waitFor();
      assert.match(await page.locator('#wiresock-remove-dialog ol').textContent(), /WireSock Secure Connect/);
      await page.keyboard.press('Escape');
      await page.locator('aside nav a').first().click();
      await page.locator('#aux-provider-wiresock').click({ timeout: 12000 });
      await page.locator('#wiresock-logo').waitFor();
      await page.locator('#wiresock-info-btn').click();
      assert.equal(await page.locator('#info-title').textContent(), 'WireSock');
      await page.locator('#info-modal-close').click();
      assert.equal(await page.locator('#warp-connect').isVisible(), false);
      assert.equal(await page.locator('#wiresock-setup').isVisible(), false);
      await page.locator('#wiresock-profile-label').filter({ hasText: 'Home VPN' }).waitFor();
      assert.equal(await page.locator('#wiresock-profile-options [role="menuitem"]').first().isVisible(), false);
      async function alignedActions(connect, status) {
        for (const [left, right] of [['#connect-btn', connect], ['#check-status-btn', status]]) {
          const a = await page.locator(left).boundingBox();
          const b = await page.locator(right).boundingBox();
          assert.ok(Math.abs(a.y + a.height - b.y - b.height) <= 1, `${left} and ${right} share a bottom edge`);
        }
      }
      await alignedActions('#wiresock-connect', '#wiresock-refresh');
      const addProfile = page.locator('#wiresock-add-profile');
      assert.equal(await page.locator('#wiresock-create').isVisible(), false);
      assert.equal(await page.locator('#wiresock-import').isVisible(), false);
      assert.ok((await addProfile.boundingBox()).x > (await page.locator('#wiresock-profile-trigger').boundingBox()).x);
      await addProfile.click();
      assert.equal(await page.locator('#wiresock-create').isVisible(), true);
      assert.equal(await page.locator('#wiresock-import').isVisible(), true);
      await page.keyboard.press('Escape');
      await page.locator('#wiresock-split-open').click();
      const splitDialog = page.locator('#wiresock-split-dialog');
      const allowed = group(splitDialog, 'allowed');
      await allowed.locator('.warp-rule-name').filter({ hasText: 'chrome.exe' }).waitFor();
      assert.equal(await splitDialog.evaluate(el => el.scrollWidth > el.clientWidth), false);
      const splitSave = page.locator('#wiresock-split-save');
      assert.equal(await splitSave.isDisabled(), true);
      await allowed.locator('.warp-rule-card button').last().click();
      await group(splitDialog, 'disallowed').locator('[data-rules-add]').click();
      await group(splitDialog, 'disallowed').locator('.warp-rule-name').filter({ hasText: 'steam.exe' }).waitFor();
      const splitSites = group(splitDialog, 'sites');
      await splitSites.locator('[data-rules-copy]').click();
      await splitSites.locator('[data-rules-copy]').click();
      await splitSites.locator('.warp-rule-card').filter({ hasText: 'example.com' }).locator('button').click();
      assert.deepEqual(await splitSites.locator('.warp-rule-name').allTextContents(), ['example.org']);
      assert.deepEqual(await page.locator('#warp-sites-list .warp-rule-name').allTextContents(), ['example.com', 'example.org']);
      await splitSave.click();
      await splitDialog.waitFor({ state: 'hidden' });
      await page.locator('#wiresock-profile-label').filter({ hasText: 'WireSock-Split-5555555555555555' }).waitFor();
      assert.equal(await page.evaluate(() => window.wire.profiles.some(profile => profile.name === 'Home VPN')), true);
      assert.equal(await page.evaluate(() => window.wire.connections), 0);
      await page.locator('#wiresock-split-open').click();
      const override = page.locator('#wiresock-global-toggle');
      await override.click();
      await group(splitDialog, 'disallowed').locator('[data-rules-add]').click();
      await group(splitDialog, 'disallowed').locator('.warp-rule-name').filter({ hasText: 'steam.exe' }).waitFor();
      await splitSave.click();
      await splitDialog.waitFor({ state: 'hidden' });
      assert.equal(await page.evaluate(() => window.wire.global.enabled), true);
      await page.locator('#wiresock-split-open').click();
      await group(splitDialog, 'disallowed').locator('.warp-rule-name').filter({ hasText: 'steam.exe' }).waitFor();
      assert.equal(await override.getAttribute('aria-pressed'), 'true');
      assert.equal(await page.locator('#wiresock-split-profile').isVisible(), false);
      await override.click();
      await splitSave.click();
      await splitDialog.waitFor({ state: 'hidden' });
      assert.deepEqual(await page.evaluate(() => window.wire.global), {
        enabled: false, applications: { allowed: [], disallowed: ['C:\\Games\\Steam\\steam.exe'] }, sites: [],
      });
      await page.locator('#wiresock-profile-trigger').click();
      const menu = page.locator('#wiresock-profile-options-list');
      const splitWidth = (await page.locator('#wiresock-split-open').boundingBox()).width;
      for (const selector of ['#wiresock-profile-trigger', '#wiresock-profile-options']) {
        assert.ok(Math.abs((await page.locator(selector).boundingBox()).width - splitWidth) < 1, `${selector} matches the split tunneling button width`);
      }
      assert.equal(await menu.evaluate(el => el.scrollHeight > el.clientHeight), true);
      assert.equal(await menu.evaluate(el => {
        const bounds = el.getBoundingClientRect();
        return bounds.top >= 0 && bounds.bottom <= innerHeight;
      }), true);
      assert.equal(await menu.locator('.ui-select-option-group:has([aria-checked="true"])').evaluate(row => {
        const button = row.querySelector('[role="menuitemradio"]');
        return getComputedStyle(row).backgroundColor !== 'rgba(0, 0, 0, 0)' && getComputedStyle(button).backgroundColor === 'rgba(0, 0, 0, 0)';
      }), true);
      await menu.hover();
      await page.mouse.wheel(0, 240);
      await page.waitForFunction(() => document.getElementById('wiresock-profile-options-list').scrollTop > 0);
      await page.keyboard.press('End');
      await page.waitForFunction(() => {
        const list = document.getElementById('wiresock-profile-options-list');
        const action = list.lastElementChild.querySelector('[role="menuitem"]');
        return document.activeElement === action && action.getBoundingClientRect().bottom <= list.getBoundingClientRect().bottom;
      });
      await page.keyboard.press('Home');
      const pick = page.locator('#wiresock-profile-options [role="menuitemradio"]').filter({ hasText: 'WARP-WireSock-1234567890ABCDEF' });
      assert.match(await pick.textContent(), /\d{1,2}:\d{2}/);
      await pick.click();
      await page.locator('#wiresock-connect').click();
      const missing = page.locator('#wiresock-setup-status');
      assert.match(await missing.textContent(), /— 11\nIPSet — 17\nhosts — 4$/);
      assert.equal(await missing.evaluate(el => getComputedStyle(el).whiteSpace), 'pre-line');
      await page.locator('#wiresock-setup-close').click();
      assert.equal(await page.evaluate(() => window.wire.state), 'disconnected');
      await page.locator('#wiresock-connect').click();
      await page.locator('#wiresock-setup-skip').click();
      await page.waitForFunction(() => document.getElementById('wiresock-connect').dataset.action === 'stop');
      assert.match(await page.locator('#header-status').textContent(), /general \(ALT12\) \+ WireSock/);
      assert.equal(await page.evaluate(() => window.wire.applied), 0);
      assert.equal(await page.evaluate(() => localStorage.getItem('zapret.wiresock.lastConnected')), 'WARP-WireSock-1234567890ABCDEF');
      await page.locator('#wiresock-refresh').click();
      await page.waitForFunction(() => document.getElementById('wiresock-status-report').getAttribute('aria-busy') === 'false');
      assert.equal(await page.locator('#wiresock-status-state').getAttribute('class'), await page.locator('#status-content > div').first().getAttribute('class'));
      assert.equal(await page.locator('#wiresock-status-profile').getAttribute('class'), await page.locator('#status-content > div').nth(1).getAttribute('class'));
      assert.match(await page.locator('#wiresock-status-profile').textContent(), /WARP-WireSock-1234567890ABCDEF/);
      assert.match(await page.locator('#wiresock-address').textContent(), /203\.0\.113\.8/);
      await page.locator('#wiresock-status-dismiss').click();
      assert.equal(await page.locator('#wiresock-status-dialog').isVisible(), false);
      await page.locator('#wiresock-refresh').click();
      await page.locator('#wiresock-status-close').click();
      await page.evaluate(() => { window.wire.failList = true; });
      await page.locator('#wiresock-connect').click();
      await page.waitForFunction(() => document.getElementById('wiresock-connect').dataset.action === 'start');
      assert.equal(await page.locator('#wiresock-error').isVisible(), false);
      await page.evaluate(() => { window.wire.failList = false; });
      await addProfile.click();
      await page.locator('#wiresock-import').click();
      await page.locator('#wiresock-profile-label').filter({ hasText: 'Imported profile' }).waitFor();
      const deleteImported = page.locator('#wiresock-profile-options button[role="menuitem"][aria-label*="Imported profile"]');
      await page.locator('#wiresock-profile-trigger').click();
      await deleteImported.click();
      await page.locator('#confirm-modal-cancel').click();
      assert.equal(await page.evaluate(() => window.wire.profiles.some(item => item.name === 'Imported profile')), true);
      await page.locator('#wiresock-profile-trigger').click();
      await deleteImported.click();
      await page.locator('#confirm-modal-ok').click();
      await page.locator('#wiresock-profile-label').filter({ hasText: 'WARP-WireSock-1234567890ABCDEF' }).waitFor();
      await addProfile.click();
      await page.locator('#wiresock-create').click();
      await page.locator('#warp-profile-terms').check();
      await page.locator('#warp-profile-save').click();
      await page.locator('#warp-profile-status').filter({ hasText: 'WARP-WireSock-5555555555555555.conf' }).waitFor();
      await page.locator('#warp-profile-close').click();
      await page.locator('#wiresock-profile-label').filter({ hasText: 'WARP-WireSock-5555555555555555' }).waitFor();
      await page.locator('#wiresock-connect').click();
      await page.locator('#wiresock-setup').click();
      await page.locator('#wiresock-setup-error').waitFor();
      await page.locator('#wiresock-setup-skip').waitFor({ state: 'hidden' });
      assert.equal(await page.evaluate(() => window.wire.state), 'disconnected');
      await page.locator('#wiresock-setup').click();
      await page.locator('#wiresock-setup').waitFor({ state: 'hidden' });
      await page.waitForFunction(() => document.getElementById('wiresock-connect').dataset.action === 'stop');
      assert.equal(await page.evaluate(() => window.wire.applied), 1);
      assert.equal(await page.evaluate(() => window.wire.restarts), 2);
      assert.equal(await page.evaluate(() => window.wire.connections), 1);
      const splitToggle = page.locator('#wiresock-split-toggle');
      const activeProfile = 'WARP-WireSock-5555555555555555';
      const profileTrigger = page.locator('#wiresock-profile-trigger');
      const homeProfile = page.getByRole('menuitemradio', { name: /Home VPN/ });
      const connectWireSock = page.locator('#wiresock-connect');
      const enabledToggle = splitToggle.and(page.locator(':enabled'));
      assert.equal(await splitToggle.getAttribute('aria-checked'), 'true');
      await splitToggle.click();
      await enabledToggle.and(page.locator('[aria-checked="false"]')).waitFor();
      assert.equal(await page.locator('#wiresock-split-toggle-hint').count(), 0);
      assert.deepEqual(await page.evaluate(() => [window.wire.active, window.wire.connectedWithSplit, window.wire.reconnections]), [activeProfile, false, 1]);
      await splitToggle.click();
      await enabledToggle.and(page.locator('[aria-checked="true"]')).waitFor();
      assert.deepEqual(await page.evaluate(() => [window.wire.active, window.wire.connectedWithSplit, window.wire.reconnections]), [activeProfile, true, 2]);
      await profileTrigger.click();
      await homeProfile.click();
      await page.locator('#wiresock-profile-label').filter({ hasText: 'Home VPN' }).waitFor();
      await enabledToggle.waitFor();
      assert.deepEqual(await page.evaluate(() => [window.wire.active, window.wire.reconnections, localStorage.getItem('zapret.wiresock.lastConnected')]), ['Home VPN', 3, 'Home VPN']);
      assert.equal(await page.locator('#wiresock-setup-dialog').isVisible(), false);
      assert.equal(await page.evaluate(() => window.wire.checks), 4);
      await connectWireSock.click();
      await connectWireSock.and(page.locator('[data-action="start"]:enabled')).waitFor();
      await splitToggle.click();
      await enabledToggle.and(page.locator('[aria-checked="false"]')).waitFor();
      await profileTrigger.click();
      await page.getByRole('menuitemradio', { name: new RegExp(activeProfile) }).click();
      await page.locator('#wiresock-profile-label').filter({ hasText: activeProfile }).waitFor();
      await enabledToggle.waitFor();
      assert.deepEqual(await page.evaluate(() => [window.wire.state, window.wire.reconnections]), ['disconnected', 3]);
      await connectWireSock.click();
      await connectWireSock.and(page.locator('[data-action="stop"]:enabled')).waitFor();
      assert.equal(await page.evaluate(() => window.wire.checks), 5);
      assert.equal(await page.evaluate(() => window.wire.connectedWithSplit), false);
      assert.deepEqual(await page.evaluate(() => window.wire.global.applications.disallowed), ['C:\\Games\\Steam\\steam.exe']);
      await page.locator('#aux-provider-warp').click();
      assert.equal(await page.locator('#warp-connect').isVisible(), true);
      assert.equal(await page.locator('#wiresock-panel').isVisible(), false);
      assert.equal(await page.locator('#warp-provider-name').textContent(), 'Cloudflare');
      await alignedActions('#warp-connect', '#warp-refresh');
      await page.evaluate(async () => {
        const { updateStatusUI } = await import('/features/status.js');
        updateStatusUI({ running: false });
      });
      assert.equal(await page.locator('#connect-temp-btn').isVisible(), true);
      await alignedActions('#warp-connect', '#warp-refresh');
      assert.deepEqual(errors, []);
      await context.close();
    }
  } finally { await browser?.close(); await server.close(); }
});
