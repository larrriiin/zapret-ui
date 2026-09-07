import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, mkdir, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { createClient, scanInstaller, securityNotes, scanRelease } from './virustotal-release.mjs';

const hash = value => createHash('sha256').update(value).digest('hex');
const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), { status, headers });
const completed = sha256 => ({ data: { attributes: { status: 'completed' } }, meta: { file_info: { sha256 } } });
async function temporary(t) {
  const dir = await mkdtemp(path.join(tmpdir(), 'zapret-vt-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return dir;
}

test('bilingual notes distinguish both installers and preserve handwritten notes on reruns', () => {
  const reports = ['EXE', 'MSI'].map(kind => ({ kind, sha256: hash(kind), url: `https://www.virustotal.com/gui/file/${hash(kind)}` }));
  const original = 'Release notes\n\n### Fixes\n- Keep this';
  const notes = securityNotes(original, reports, 'larrriiin/zapret-ui');
  assert.ok(notes.startsWith(original));
  for (const heading of ['### Безопасность', '### Security']) assert.ok(notes.includes(heading));
  for (const kind of ['EXE', 'MSI']) assert.ok(notes.includes(`(${kind}):** \`${hash(kind)}\``));
  assert.equal(securityNotes(notes, reports, 'larrriiin/zapret-ui'), notes);
  assert.throws(() => securityNotes('<!-- zapret-virustotal:start -->', reports, 'a/b'), /incomplete/);
});

test('public API calls are spaced and 429 retries back off without exposing the key', async () => {
  let clock = 0;
  const calls = [];
  const client = createClient('secret', { now: () => clock, wait: async ms => { clock += ms; },
    fetchImpl: async (_, options) => {
      calls.push(clock);
      assert.equal(options.headers['x-apikey'], 'secret');
      assert.equal(options.redirect, 'error');
      return calls.length === 2 ? json({}, 429, { 'retry-after': '80' }) : json({ ok: true });
    } });
  await client('https://www.virustotal.com/api/v3/files');
  await client('https://www.virustotal.com/api/v3/files');
  assert.deepEqual(calls, [0, 16000, 96000]);
  await assert.rejects(client('https://evil.test/upload'), /unexpected upload URL/);
  assert.throws(() => createClient(''), /VIRUSTOTAL_API_KEY/);
  const fail = createClient('secret', { wait: async () => {}, fetchImpl: async () => json({ key: 'secret' }, 401) });
  await assert.rejects(fail('https://www.virustotal.com/api/v3/files'), error => /401/.test(error.message) && !error.message.includes('secret'));
});

test('upload waits for completion and checks SHA-256; pending and mismatched results fail', async t => {
  const file = path.join(await temporary(t), 'setup.exe');
  await writeFile(file, 'installer');
  let calls = 0;
  const request = async (url, options) => {
    calls++;
    if (options?.method === 'POST') {
      assert.equal(await options.body.get('file').text(), 'installer');
      return { data: { id: 'analysis/id' } };
    }
    assert.ok(url.endsWith('/analyses/analysis%2Fid'));
    return calls === 2 ? { data: { attributes: { status: 'queued' } } } : completed(hash('installer'));
  };
  const report = await scanInstaller(file, request, { wait: async () => {} });
  assert.equal(report.sha256, hash('installer'));
  assert.equal(calls, 3);
  await assert.rejects(scanInstaller(file, async (_, options) => options ? { data: { id: 'id' } } : completed(hash('wrong')), { wait: async () => {} }), /does not match/);
  await assert.rejects(scanInstaller(file, async (_, options) => options ? { data: { id: 'id' } } : { data: { attributes: { status: 'queued' } } }, { wait: async () => {}, maxPolls: 2 }), /pending/);
});

test('large installers use the dedicated upload URL', async t => {
  const file = path.join(await temporary(t), 'large.msi');
  const bytes = Buffer.alloc(32 * 1024 * 1024 + 1, 1);
  await writeFile(file, bytes);
  const calls = [];
  await scanInstaller(file, async (url, options) => {
    calls.push(url);
    if (url.endsWith('upload_url')) return { data: 'https://www.virustotal.com/upload/one-use' };
    if (options?.method === 'POST') return { data: { id: 'large' } };
    return completed(hash(bytes));
  }, { wait: async () => {} });
  assert.ok(calls[0].endsWith('/files/upload_url'));
  assert.equal(calls[1], 'https://www.virustotal.com/upload/one-use');
});

test('release integration scans exact bundles, checks published digests, then edits notes once', async t => {
  const dir = await temporary(t);
  const assets = [];
  for (const [folder, name] of [['nsis', 'ZAPRET UI_1.2.3_x64-setup.exe'], ['msi', 'ZAPRET UI_1.2.3_x64_en-US.msi']]) {
    await mkdir(path.join(dir, folder));
    await writeFile(path.join(dir, folder, name), name);
    assets.push({ name: name.replaceAll(' ', '.'), digest: `sha256:${hash(name)}` });
  }
  let lastHash;
  let edits = [];
  const options = { apiKey: 'secret', version: '1.2.3', releaseId: 42, bundleDirectory: dir,
    context: { repo: { owner: 'larrriiin', repo: 'zapret-ui' } }, core: { info() {} },
    clientOptions: { wait: async () => {}, fetchImpl: async (_, options) => {
      if (options.method === 'POST') {
        lastHash = hash(await options.body.get('file').text());
        return json({ data: { id: 'id' } });
      }
      return json(completed(lastHash));
    } }, scanOptions: { wait: async () => {} },
    github: { rest: { repos: {
      getRelease: async () => ({ data: { body: 'Keep release notes', assets } }),
      updateRelease: async update => { edits.push(update); },
    } } },
  };
  await scanRelease(options);
  assert.equal(edits.length, 1);
  assert.equal(edits[0].release_id, 42);
  assert.ok(edits[0].body.startsWith('Keep release notes'));
  assert.equal(edits[0].draft, undefined);
  edits = [];
  assets[0].digest = `sha256:${hash('different release asset')}`;
  await assert.rejects(scanRelease(options), /does not match/);
  assert.equal(edits.length, 0);
});

test('workflow requires secret, scans before publication, and never overwrites notes afterward', async () => {
  const workflow = await readFile(new URL('../.github/workflows/releaser.yml', import.meta.url), 'utf8');
  assert.ok(workflow.indexOf('Check VirusTotal configuration') < workflow.indexOf('Resolve target release'));
  assert.ok(workflow.indexOf('await scanRelease') < workflow.indexOf('--draft=false'));
  assert.ok(!workflow.includes('--notes'));
});
