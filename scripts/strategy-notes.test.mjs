import assert from 'node:assert/strict';
import test from 'node:test';
import { getStrategyNote, readStrategyNotes, saveStrategyNote, MAX_STRATEGY_NOTE_LENGTH } from '../src/features/strategy-notes.js';

function storage() {
  const values = new Map([['zapret.favorites', '["general (ALT12)"]']]);
  return {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
}

test('notes survive reloads, core list replacement and rollback without pruning absent strategies', () => {
  const profile = storage();
  saveStrategyNote('general (ALT12)', 'Работает вечером', profile);
  saveStrategyNote('custom', 'Моя стратегия', profile);
  // Reading a new list must not remove annotations for strategies absent from that release.
  assert.equal(getStrategyNote('general', readStrategyNotes(profile)), '');
  saveStrategyNote('general', 'После обновления', profile);
  const restartedProfile = storage();
  restartedProfile.setItem('zapret.strategyNotes', profile.getItem('zapret.strategyNotes'));
  assert.equal(getStrategyNote('general (ALT12)', readStrategyNotes(restartedProfile)), 'Работает вечером');
  assert.equal(getStrategyNote('custom', readStrategyNotes(restartedProfile)), 'Моя стратегия');
  assert.equal(profile.getItem('zapret.favorites'), '["general (ALT12)"]');
});

test('edits and deletion affect only the requested strategy and tolerate case and BAT suffix changes', () => {
  const profile = storage();
  saveStrategyNote('general (ALT12)', 'First', profile);
  saveStrategyNote('general', 'Keep', profile);
  saveStrategyNote(' GENERAL (alt12).BAT ', '  Новая\nзаметка  ', profile);
  assert.equal(getStrategyNote('general (ALT12)', readStrategyNotes(profile)), 'Новая заметка');
  saveStrategyNote('GENERAL (ALT12)', '', profile);
  assert.equal(getStrategyNote('general (ALT12)', readStrategyNotes(profile)), '');
  assert.equal(getStrategyNote('general', readStrategyNotes(profile)), 'Keep');
});

test('unreadable or unsupported data is never overwritten by an edit', () => {
  const profile = storage();
  for (const raw of ['broken json', '{"version":2,"notes":{"general":"Keep"}}', '{"version":1,"notes":{"general":42}}']) {
    profile.setItem('zapret.strategyNotes', raw);
    assert.deepEqual(Object.keys(readStrategyNotes(profile)), []);
    assert.throws(() => saveStrategyNote('general', 'New', profile));
    assert.equal(profile.getItem('zapret.strategyNotes'), raw);
  }
});

test('rejects oversized notes and reports write failures without losing previously saved notes', () => {
  const profile = storage();
  saveStrategyNote('general', 'Keep', profile);
  assert.throws(() => saveStrategyNote('general', 'x'.repeat(MAX_STRATEGY_NOTE_LENGTH + 1), profile));
  const blocked = { getItem: profile.getItem, setItem() { throw new Error('quota'); } };
  assert.throws(() => saveStrategyNote('general', 'New', blocked));
  assert.equal(getStrategyNote('general', readStrategyNotes(profile)), 'Keep');
});
