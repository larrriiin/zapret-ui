import assert from 'node:assert/strict';
import test from 'node:test';
import { state } from '../src/lib/state.js';
import { handleGameFilterChange, handleIPSetFilterChange } from '../src/features/filters.js';
import { handleFakeChange, updateFakesUI } from '../src/features/fakes.js';

function fixture() {
  const calls = [];
  const filters = { game_filter: 'all', ipset: 'loaded' };
  const fakes = { current_discord_fake: 'discord-old', current_game_fake: 'game-old', available_fakes: [] };
  let failFake = false;
  globalThis.document = { getElementById: () => null };
  globalThis.window = { __TAURI__: { core: { invoke: async (command, args) => {
    calls.push([command, args]);
    switch (command) {
      case 'set_game_filter': filters.game_filter = args.mode; break;
      case 'set_ipset_filter': filters.ipset = args.mode; break;
      case 'get_filters_status': return structuredClone(filters);
      case 'set_active_fake':
        if (failFake) throw new Error('write failed');
        fakes[args.fakeType === 'discord' ? 'current_discord_fake' : 'current_game_fake'] = args.fakeName;
        break;
      case 'get_fakes_info': return structuredClone(fakes);
      case 'get_zapret_status': return { running: true, strategy: 'general', mode: 'service' };
      case 'stop_zapret': case 'start_zapret': break;
      default: throw new Error(`Unexpected command: ${command}`);
    }
  } } } };
  state.currentFilters = structuredClone(filters);
  updateFakesUI(fakes);
  return { calls, filters, setFailFake(value) { failFake = value; } };
}

test('reselecting any active filter or fake neither writes settings nor interrupts a running ZAPRET', async () => {
  const { calls } = fixture();
  for (const mode of ['disabled', 'all', 'tcp', 'udp']) {
    state.currentFilters.game_filter = mode;
    await handleGameFilterChange(mode);
  }
  for (const mode of ['none', 'any', 'loaded']) {
    state.currentFilters.ipset = mode;
    await handleIPSetFilterChange(mode);
  }
  await handleFakeChange('discord', 'discord-old');
  await handleFakeChange('game', 'game-old');
  assert.deepEqual(calls, []);
});

test('a changed choice still saves and restarts once; repeating it does nothing', async () => {
  const { calls } = fixture();
  for (const [change, value] of [
    [() => handleGameFilterChange('tcp'), 'set_game_filter'],
    [() => handleIPSetFilterChange('any'), 'set_ipset_filter'],
    [() => handleFakeChange('discord', 'discord-new'), 'set_active_fake'],
    [() => handleFakeChange('game', 'game-new'), 'set_active_fake'],
  ]) {
    calls.length = 0;
    await change();
    assert.equal(calls.filter(([command]) => command === value).length, 1);
    assert.equal(calls.filter(([command]) => command === 'stop_zapret').length, 1);
    assert.equal(calls.filter(([command]) => command === 'start_zapret').length, 1);
    calls.length = 0;
    await change();
    assert.deepEqual(calls, []);
  }
});

test('a rejected fake change remains retryable and never restarts ZAPRET', async t => {
  const { calls, setFailFake } = fixture();
  t.mock.method(console, 'error', () => {});
  setFailFake(true);
  await handleFakeChange('discord', 'discord-new');
  await handleFakeChange('discord', 'discord-new');
  assert.deepEqual(calls.map(([command]) => command), ['set_active_fake', 'set_active_fake']);
  calls.length = 0;
  await handleFakeChange('discord', 'discord-old');
  assert.deepEqual(calls, []);
});
