import assert from 'node:assert/strict';
import test from 'node:test';

import { chooseStrategy, readRememberedStrategy } from '../src/features/strategies.js';

test('restores an available remembered strategy when zapret is stopped', () => {
  const storage = { getItem: (key) => key === 'zapret.lastStrategy' ? 'general ALT' : null };
  const remembered = readRememberedStrategy(storage);

  assert.equal(
    chooseStrategy(['general', 'general ALT'], '', '', remembered),
    'general ALT',
  );
});

test('keeps explicit and current choices ahead of persisted state', () => {
  const strategies = ['general', 'general ALT', 'custom'];

  assert.equal(chooseStrategy(strategies, 'custom', 'general ALT', 'general'), 'custom');
  assert.equal(chooseStrategy(strategies, '', 'general ALT', 'general'), 'general ALT');
});

test('falls back safely when the remembered strategy no longer exists', () => {
  assert.equal(chooseStrategy(['general', 'ALT'], '', '', 'removed'), 'general');
  assert.equal(chooseStrategy(['ALT'], '', '', 'removed'), 'ALT');
  assert.equal(chooseStrategy([], '', '', 'removed'), '');
});

test('ignores unavailable storage', () => {
  const storage = { getItem: () => { throw new Error('blocked'); } };
  assert.equal(readRememberedStrategy(storage), '');
});
