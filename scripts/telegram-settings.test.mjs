import test from 'node:test';
import assert from 'node:assert/strict';
import { parseDcIps, initTelegramSettings, refreshTelegramSettings, setTelegramSettingsBusy, saveTelegramAutostart } from '../src/features/telegram-settings.js';
import { checkTelegramUpdate } from '../src/features/telegram-updates.js';

test('DC editor accepts common separators and rejects duplicate/invalid addresses', () => {
  assert.deepEqual(parseDcIps('2:149.154.167.220\n4 → 149.154.167.221'), {'2':'149.154.167.220','4':'149.154.167.221'});
  assert.deepEqual(parseDcIps(''), {});
  for (const text of ['2:999.0.0.1', '0:1.1.1.1', '2:1.1.1.1\n2:1.0.0.1', '2:example.com', '32768:1.1.1.1']) assert.throws(() => parseDcIps(text));
});
test('uninstalled module never triggers a remote update check', async () => {
  const calls = [];
  globalThis.window = {__TAURI__:{core:{invoke:async command => {calls.push(command);return {installed:false};}}}};
  assert.equal(await checkTelegramUpdate(), null);
  assert.deepEqual(calls, ['get_telegram_status']);
});
test('installed module reports update endpoint failure without claiming up-to-date', async () => {
  globalThis.window = {__TAURI__:{core:{invoke:async command => {if(command==='get_telegram_status')return {installed:true,version:'1.10.2'}; throw new Error('offline');}}}};
  const result = await checkTelegramUpdate();
  assert.equal(result.installed, true); assert.equal(result.current, '1.10.2'); assert.match(result.error, /offline/);
});

test('autostart saves immediately while running, preserves invalid draft fields and dirty state, and rolls back errors', async () => {
  const ids = ['telegram-host','telegram-port','telegram-secret','telegram-start-with-zapret','telegram-dc-ips',
    'telegram-cfproxy','telegram-cf-domains','telegram-worker','telegram-worker-domains','telegram-save',
    'telegram-unsaved','telegram-config-fields','telegram-settings-lock','telegram-message','telegram-autostart-status',
    'telegram-info-dialog','telegram-info-close','telegram-port-form','telegram-regenerate'];
  const nodes = Object.fromEntries(ids.map(id => [id, { value: '', checked: false, disabled: false, dataset: {}, hidden: false,
    listeners: {}, addEventListener(type, handler) { this.listeners[type] = handler; },
    nextElementSibling: { classList: { toggle() {} } } }]));
  globalThis.document = { getElementById: id => nodes[id], querySelectorAll: () => [], body: { append() {} } };
  let config = { host: '127.0.0.1', port: 1443, secret: 'secret', start_with_zapret: false, dc_ips: { 2: '1.1.1.1' }, cfproxy: true, cfproxy_domains: [], worker: false, worker_domains: [] };
  const calls = []; let rejectSave = false; let finish;
  globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
    if (command === 'get_telegram_config') return structuredClone(config);
    assert.equal(command, 'set_telegram_autostart'); calls.push(args);
    await new Promise(resolve => { finish = resolve; });
    if (rejectSave) throw new Error('disk full');
    config.start_with_zapret = args.enabled; return args.enabled;
  } } } };
  await refreshTelegramSettings(false); await refreshTelegramSettings(true);
  initTelegramSettings(() => { throw new Error('autostart used the whole-form save action'); });
  setTelegramSettingsBusy(false, false, false);
  nodes['telegram-host'].value = 'unsaved.invalid'; nodes['telegram-port'].value = 'invalid';
  nodes['telegram-dc-ips'].value = 'invalid draft'; nodes['telegram-worker-domains'].value = 'unfinished';
  nodes['telegram-host'].listeners.input();
  assert.equal(nodes['telegram-save'].disabled, false); assert.equal(nodes['telegram-unsaved'].hidden, false);
  // The flag remains editable when other settings are locked by a running proxy.
  setTelegramSettingsBusy(true, true, false);
  assert.equal(nodes['telegram-start-with-zapret'].disabled, false);
  nodes['telegram-start-with-zapret'].checked = true;
  const pending = saveTelegramAutostart();
  assert.equal(nodes['telegram-start-with-zapret'].disabled, true);
  await saveTelegramAutostart(); assert.equal(calls.length, 1);
  finish(); await pending;
  assert.deepEqual(calls, [{ enabled: true }]);
  assert.equal(config.host, '127.0.0.1'); assert.equal(config.port, 1443);
  assert.equal(nodes['telegram-host'].value, 'unsaved.invalid'); assert.equal(nodes['telegram-port'].value, 'invalid');
  assert.equal(nodes['telegram-dc-ips'].value, 'invalid draft'); assert.equal(nodes['telegram-worker-domains'].value, 'unfinished');
  setTelegramSettingsBusy(false, false, false);
  assert.equal(nodes['telegram-save'].disabled, false); assert.equal(nodes['telegram-unsaved'].hidden, false);
  rejectSave = true; nodes['telegram-start-with-zapret'].checked = false;
  const failed = saveTelegramAutostart(); finish(); await failed;
  assert.equal(nodes['telegram-start-with-zapret'].checked, true);
  assert.equal(nodes['telegram-autostart-status'].dataset.state, 'error');
  assert.equal(nodes['telegram-port'].value, 'invalid'); assert.equal(nodes['telegram-unsaved'].hidden, false);
  // A fresh session reads the persisted flag.
  await refreshTelegramSettings(false); await refreshTelegramSettings(true);
  assert.equal(nodes['telegram-start-with-zapret'].checked, true);
});
