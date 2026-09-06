import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareManifest, createManifestFromBundle } from './prepare-installer-update.mjs';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import os from 'node:os';
import path from 'node:path';

const options = { version: '26.9.2', tag: 'v26.9.2', signature: 'dGVzdA==\n', repository: 'owner/zapret-ui', assetName: 'ZAPRET UI_26.9.2_x64-setup.exe' };
const original = {
  version: '26.9.2', notes: 'Release notes', pub_date: '2026-09-05T00:00:00Z',
  platforms: {
    'windows-x86_64': { url: 'old.exe', signature: 'old' },
    'windows-x86_64-nsis': { url: 'old-nsis.exe', signature: 'old' },
    'windows-x86_64-msi': { url: 'old.msi', signature: 'msi' },
    'linux-x86_64': { url: 'app.AppImage', signature: 'linux' },
  },
};
test('legacy and current EXE clients receive the signed native NSIS; MSI and other platforms are preserved', () => {
  const result = prepareManifest(original, options);
  assert.equal(result.platforms['windows-x86_64'].url, 'https://github.com/owner/zapret-ui/releases/download/v26.9.2/ZAPRET.UI_26.9.2_x64-setup.exe');
  assert.equal(result.platforms['windows-x86_64'].signature, 'dGVzdA==');
  assert.deepEqual(result.platforms['windows-x86_64'], result.platforms['windows-x86_64-nsis']);
  assert.deepEqual(result.platforms['windows-x86_64-msi'], original.platforms['windows-x86_64-msi']);
  assert.deepEqual(result.platforms['linux-x86_64'], original.platforms['linux-x86_64']);
  assert.equal(result.notes, original.notes);
  assert.equal(result.pub_date, original.pub_date);
  assert.equal(original.platforms['windows-x86_64'].url, 'old.exe');
});
test('creates latest.json data without a pre-existing manifest and retains the signed MSI target', async t => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'zapret-manifest-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await mkdir(path.join(directory, 'msi'));
  const msiName = 'ZAPRET UI_26.9.2_x64_en-US.msi';
  await writeFile(path.join(directory, 'msi', msiName), 'test package');
  await writeFile(path.join(directory, 'msi', `${msiName}.sig`), 'bXNp\n');
  const result = await createManifestFromBundle(directory, options, { requireMsi: true });
  assert.equal(result.version, options.version);
  assert.ok(Number.isFinite(Date.parse(result.pub_date)));
  assert.equal(result.platforms['windows-x86_64'].signature, 'dGVzdA==');
  assert.deepEqual(result.platforms['windows-x86_64-nsis'], result.platforms['windows-x86_64']);
  assert.equal(result.platforms['windows-x86_64-msi'].signature, 'bXNp');
  assert.ok(result.platforms['windows-x86_64-msi'].url.endsWith('/ZAPRET.UI_26.9.2_x64_en-US.msi'));
});
test('fails before publication if required MSI or its signature is missing', async t => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'zapret-manifest-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await assert.rejects(createManifestFromBundle(directory, options, { requireMsi: true }), /MSI updater package is missing/);
  const nsisOnly = await createManifestFromBundle(directory, options);
  assert.equal(Object.keys(nsisOnly.platforms).length, 2);
  await mkdir(path.join(directory, 'msi'));
  await writeFile(path.join(directory, 'msi', 'ZAPRET UI_26.9.2_x64_en-US.msi'), 'test package');
  await assert.rejects(createManifestFromBundle(directory, options, { requireMsi: true }), /ENOENT/);
});
test('workflow CLI creates the missing latest.json from local signed artifacts', async t => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'zapret-manifest-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const { version } = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const bundle = path.join(directory, 'bundle');
  const installer = path.join(directory, `ZAPRET UI_${version}_x64-setup.exe`);
  const output = path.join(directory, 'latest.json');
  await mkdir(path.join(bundle, 'msi'), { recursive: true });
  await writeFile(installer, 'test package');
  await writeFile(`${installer}.sig`, options.signature);
  const msi = path.join(bundle, 'msi', `ZAPRET UI_${version}_x64_en-US.msi`);
  await writeFile(msi, 'test package');
  await writeFile(`${msi}.sig`, 'bXNp');
  execFileSync(process.execPath, [fileURLToPath(new URL('./prepare-installer-update.mjs', import.meta.url)), output, installer, '--from-bundle', bundle], {
    // A manual run uses a branch ref; release URLs must use the requested tag.
    env: { ...process.env, GITHUB_REPOSITORY: options.repository, GITHUB_REF_NAME: 'main', RELEASE_TAG: `v${version}` },
  });
  const result = JSON.parse(await readFile(output, 'utf8'));
  assert.equal(result.version, version);
  assert.ok(result.platforms['windows-x86_64'].url.includes(`/download/v${version}/`));
  assert.equal(Object.keys(result.platforms).length, 3);
  assert.ok(result.platforms['windows-x86_64'].url.endsWith(`/ZAPRET.UI_${version}_x64-setup.exe`));
});
test('does not prepare an update with mismatched version, missing signature or wrong release', () => {
  for (const patch of [{ version: '26.9.3' }, { signature: '' }, { tag: 'v0.0.1' }, { repository: '../repo' }, { assetName: 'app.msi' }, { assetName: 'ZAPRET_26.9.2_x64-branded-setup.exe' }, { assetName: 'ZAPRET_26.9.1_x64-setup.exe' }]) {
    assert.throws(() => prepareManifest(original, { ...options, ...patch }));
  }
  assert.throws(() => prepareManifest({ version: '26.9.2', platforms: { 'linux-x86_64': {} } }, options));
});
