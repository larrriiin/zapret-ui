import assert from 'node:assert/strict';
import test from 'node:test';
import { compareVersions, normalizeVersion } from './flowseal-version.mjs';

test('compares a patch update as newer', () => {
  assert.equal(compareVersions('1.10.1', '1.10.0'), 1);
});

test('compares identical versions as equal', () => {
  assert.equal(compareVersions('1.10.0', '1.10.0'), 0);
});

test('compares numeric components instead of strings', () => {
  assert.equal(compareVersions('1.9.9', '1.10.0'), -1);
});

test('removes one leading v', () => {
  assert.equal(normalizeVersion('v1.10.1'), '1.10.1');
});

test('returns unknown for a nonstandard version', () => {
  assert.equal(normalizeVersion('1.10.1-beta.1'), null);
  assert.equal(compareVersions('not-a-version', '1.10.0'), null);
});
