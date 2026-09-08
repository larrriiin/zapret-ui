import test from 'node:test';
import assert from 'node:assert/strict';
import { parseDcIps } from '../src/features/telegram-settings.js';
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
