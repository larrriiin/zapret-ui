import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareManifest } from './prepare-installer-update.mjs';

const options = { version: '26.9.2', tag: 'v26.9.2', signature: 'dGVzdA==\n', repository: 'owner/zapret-ui', assetName: 'ZAPRET_26.9.2_x64-branded-setup.exe' };
const original = {
  version: '26.9.2', notes: 'Release notes', pub_date: '2026-09-05T00:00:00Z',
  platforms: {
    'windows-x86_64': { url: 'old.exe', signature: 'old' },
    'windows-x86_64-nsis': { url: 'old-nsis.exe', signature: 'old' },
    'windows-x86_64-msi': { url: 'old.msi', signature: 'msi' },
    'linux-x86_64': { url: 'app.AppImage', signature: 'linux' },
  },
};
test('legacy and current EXE clients receive the signed wrapper; MSI and other platforms are preserved', () => {
  const result = prepareManifest(original, options);
  assert.equal(result.platforms['windows-x86_64'].url, 'https://github.com/owner/zapret-ui/releases/download/v26.9.2/ZAPRET_26.9.2_x64-branded-setup.exe');
  assert.equal(result.platforms['windows-x86_64'].signature, 'dGVzdA==');
  assert.deepEqual(result.platforms['windows-x86_64'], result.platforms['windows-x86_64-nsis']);
  assert.deepEqual(result.platforms['windows-x86_64-msi'], original.platforms['windows-x86_64-msi']);
  assert.deepEqual(result.platforms['linux-x86_64'], original.platforms['linux-x86_64']);
  assert.equal(result.notes, original.notes);
  assert.equal(result.pub_date, original.pub_date);
  assert.equal(original.platforms['windows-x86_64'].url, 'old.exe');
});
test('does not prepare an update with mismatched version, missing signature or wrong release', () => {
  for (const patch of [{ version: '26.9.3' }, { signature: '' }, { tag: 'v0.0.1' }, { repository: '../repo' }, { assetName: 'app.msi' }]) {
    assert.throws(() => prepareManifest(original, { ...options, ...patch }));
  }
  assert.throws(() => prepareManifest({ version: '26.9.2', platforms: { 'linux-x86_64': {} } }, options));
});
