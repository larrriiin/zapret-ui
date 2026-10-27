import test from 'node:test';
import assert from 'node:assert/strict';
import { availableUpdateVersions, createUpdateNotifier } from '../src/features/update-notifications.js';
import { checkForUpdates } from '../src/features/updates.js';

const component = (latest, extra = {}) => ({ available: true, latest, ...extra });
test('only reliable component versions become notifications', () => {
  assert.deepEqual(availableUpdateVersions({ ui: component('27.1'), core: component(null), telegram: component('2', { error: 'offline' }) }), { ui: '27.1' });
});
test('groups components, remembers versions across launches, re-notifies only for a new version', async () => {
  let saved = null; const deliveries = [];
  const options = { read: () => saved, write: value => { saved = value; }, deliver: async (keys, data, { showNative }) => { if (showNative) deliveries.push(keys); return true; } };
  const notify = createUpdateNotifier(options);
  assert.equal(await notify({ ui: component('27.1') }), true);
  assert.deepEqual(deliveries, [['ui']]);
  assert.equal(await notify({ ui: component('27.1') }), false);
  const afterRestart = createUpdateNotifier(options);
  assert.equal(await afterRestart({ ui: component('27.1') }), false);
  assert.equal(await afterRestart({ ui: component('27.1'), core: component('1.12'), telegram: component('2.1') }), true);
  assert.deepEqual(deliveries[1], ['ui', 'core', 'telegram']);
  assert.equal(await afterRestart({ ui: component('27.2'), core: component('1.12') }), true);
  assert.equal(deliveries.length, 3);
});
test('offline component cannot hide a healthy component; failed delivery remains retryable', async () => {
  let fail = true, attempts = 0;
  const notify = createUpdateNotifier({ read: () => '{invalid', write: () => {}, deliver: async keys => {
    attempts++; assert.deepEqual(keys, ['telegram']); if (fail) throw new Error('notification unavailable'); return true;
  } });
  const data = { ui: { error: 'offline' }, telegram: component('2.1') };
  await assert.rejects(notify(data), /unavailable/); fail = false;
  assert.equal(await notify(data), true); assert.equal(attempts, 2);
});
test('hidden-window startup sends a native notification, with no background modal and no overlapping checks', async () => {
  const calls = []; let fetches = 0; let version = '27.1'; const storage = new Map();
  globalThis.localStorage = { getItem: key => storage.get(key), setItem: (key, value) => storage.set(key, value) };
  globalThis.document = { getElementById: () => null, createElement: () => { throw new Error('background check opened a modal'); } };
  globalThis.window = { __TAURI__: {
    updater: { check: async () => { fetches++; await new Promise(resolve => setTimeout(resolve, 10)); return { version }; } },
    core: { invoke: async (command, args) => {
      calls.push([command, args]);
      if (command === 'get_ui_version_cmd') return '26.10.3';
      if (command === 'get_local_version_cmd') return '1.10';
      if (command === 'get_core_update_info') throw new Error('offline core');
      if (command === 'get_telegram_status') return { installed: false };
      if (command === 'notify_available_updates') return true;
      throw new Error(command);
    } },
  } };
  await Promise.all([checkForUpdates(false), checkForUpdates(false)]);
  assert.equal(fetches, 1);
  const shown = () => calls.filter(([command, args]) => command === 'notify_available_updates' && args.showNotification);
  assert.equal(shown().length, 1);
  await checkForUpdates(false);
  assert.equal(shown().length, 1);
  version = '27.2'; await checkForUpdates(false);
  const notifications = shown();
  assert.equal(notifications.length, 2);
  assert.match(notifications[0][1].body, /ZAPRET UI/);
  assert.match(notifications[0][1].body, /трея/);
});

test('foreground uses the full modal independently of earlier Windows notifications and suppresses repeats within a session', async () => {
  let saved = null, hidden = true, modals = 0, native = 0;
  const options = { read: () => saved, write: value => { saved = value; }, deliver: async (keys, data, { showNative, showModal }) => {
    if (hidden && showNative) native++;
    if (!hidden && showModal) modals++;
    return hidden;
  } };
  const data = { ui: component('27.1'), core: component('1.12'), telegram: component('2.1') };
  const notify = createUpdateNotifier(options);
  await notify(data); assert.equal(native, 1); assert.equal(modals, 0);
  hidden = false;
  assert.equal(await notify(data), true); assert.equal(modals, 1);
  assert.equal(await notify(data), false); assert.equal(modals, 1);
  const afterRestart = createUpdateNotifier(options);
  assert.equal(await afterRestart(data), true); assert.equal(modals, 2);
  assert.equal(await afterRestart({ ...data, ui: component('27.2') }), true); assert.equal(modals, 3);
  hidden = true; await afterRestart({ ...data, ui: component('27.2') }); assert.equal(native, 1);
});

test('network failures never open a modal or send Windows notifications; a missing core only opens the visible setup modal', async () => {
  let deliveries = 0, hidden = true;
  const notify = createUpdateNotifier({ read: () => null, write: () => {}, deliver: async (keys, data, options) => {
    deliveries++; assert.deepEqual(keys, []); assert.equal(options.showNative, false); return hidden;
  } });
  assert.equal(await notify({ ui: { error: 'offline' }, core: { available: false, status: 'unknown' } }), false);
  assert.equal(deliveries, 0);
  const missing = { core: component('1.12', { status: 'not_installed', error: true }) };
  assert.equal(await notify(missing), false);
  hidden = false; assert.equal(await notify(missing), true);
});
