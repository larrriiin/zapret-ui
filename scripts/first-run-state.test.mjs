import assert from 'node:assert/strict';
import test from 'node:test';

import { SETUP_COMPLETE_KEY, shouldShowFirstRun } from '../src/features/setup-state.js';

function storage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    value: key => values.get(key),
  };
}

test('an installed core suppresses first-run and repairs a missing completion marker', async () => {
  const state = storage();
  assert.equal(await shouldShowFirstRun({ ensureBinaries: async () => true, storage: state }), false);
  assert.equal(state.value(SETUP_COMPLETE_KEY), '1');
});

test('a missing or invalid core still opens setup', async () => {
  assert.equal(await shouldShowFirstRun({ ensureBinaries: async () => false, storage: storage() }), true);
});

test('a transient core-check error does not reopen setup for a configured user', async () => {
  const warnings = [];
  const state = storage({ [SETUP_COMPLETE_KEY]: '1' });
  assert.equal(await shouldShowFirstRun({
    ensureBinaries: async () => { throw new Error('temporary failure'); },
    storage: state,
    warn: (...args) => warnings.push(args),
  }), false);
  assert.equal(warnings.length, 1);
});
