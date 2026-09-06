import assert from 'node:assert/strict';
import test from 'node:test';

import { checkForUpdates, checkUIUpdate } from '../src/features/updates.js';

test('background update checks never resolve or use the built-in proxy', async () => {
  const updaterCalls = [];
  const invokeCalls = [];
  const warnings = [];
  const originalWarn = console.warn;
  globalThis.document = { getElementById: () => null };
  globalThis.window = {
    __TAURI__: {
      updater: {
        check: async (options) => {
          updaterCalls.push(options);
          throw new Error('offline');
        },
      },
      core: {
        invoke: async (command, args) => {
          invokeCalls.push({ command, args });
          if (command === 'get_ui_version_cmd') return '26.9.5';
          if (command === 'get_core_update_info') throw new Error('offline');
          if (command === 'get_update_proxy') throw new Error('background check requested proxy');
          throw new Error(`Unexpected command: ${command}`);
        },
      },
    },
  };

  console.warn = (...args) => warnings.push(args);
  try {
    await checkForUpdates(false);
  } finally {
    console.warn = originalWarn;
  }

  assert.equal(warnings.length, 1);
  assert.deepEqual(updaterCalls, [undefined]);
  assert.deepEqual(invokeCalls, [
    { command: 'get_ui_version_cmd', args: undefined },
    { command: 'get_core_update_info', args: { useProxy: false, customProxy: null } },
  ]);
});

test('the built-in proxy is resolved only for an explicit proxy retry', async () => {
  const updaterCalls = [];
  const invokeCalls = [];
  globalThis.window = {
    __TAURI__: {
      updater: {
        check: async (options) => {
          updaterCalls.push(options);
          return null;
        },
      },
      core: {
        invoke: async (command) => {
          invokeCalls.push(command);
          assert.equal(command, 'get_update_proxy');
          return 'socks5://project-proxy.invalid:1080';
        },
      },
    },
  };

  await checkUIUpdate(true);

  assert.deepEqual(invokeCalls, ['get_update_proxy']);
  assert.deepEqual(updaterCalls, [{ proxy: 'socks5://project-proxy.invalid:1080' }]);
});

test('a user-supplied proxy does not resolve the built-in proxy', async () => {
  const updaterCalls = [];
  globalThis.window = {
    __TAURI__: {
      updater: {
        check: async (options) => {
          updaterCalls.push(options);
          return null;
        },
      },
      core: {
        invoke: async () => {
          throw new Error('custom proxy retry requested built-in proxy');
        },
      },
    },
  };

  await checkUIUpdate(true, 'http://user-proxy.invalid:8080');

  assert.deepEqual(updaterCalls, [{ proxy: 'http://user-proxy.invalid:8080' }]);
});
