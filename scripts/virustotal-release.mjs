import { createHash } from 'node:crypto';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';

const API = 'https://www.virustotal.com/api/v3';
const START = '<!-- zapret-virustotal:start -->';
const END = '<!-- zapret-virustotal:end -->';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export function createClient(apiKey, { fetchImpl = fetch, wait = sleep, now = Date.now } = {}) {
  if (!apiKey?.trim()) throw new Error('Add the VIRUSTOTAL_API_KEY repository secret before publishing.');
  let nextRequest = 0;
  return async function request(url, options = {}) {
    const target = new URL(url);
    if (target.protocol !== 'https:' || target.hostname !== 'www.virustotal.com' || target.port || target.username || target.password) {
      throw new Error('VirusTotal returned an unexpected upload URL.');
    }
    for (let attempt = 0; attempt < 3; attempt++) {
      await wait(Math.max(0, nextRequest - now()));
      nextRequest = now() + 16_000;
      let response;
      try {
        response = await fetchImpl(url, { ...options, redirect: 'error',
          headers: { 'x-apikey': apiKey }, signal: AbortSignal.timeout(180_000) });
      } catch {
        // Never print the request, headers, API key, or temporary upload URL.
        throw new Error('VirusTotal network request failed or timed out. Rerun the release workflow.');
      }
      if (response.status === 429 || response.status >= 500) {
        if (attempt === 2) throw new Error(`VirusTotal unavailable or quota exceeded (HTTP ${response.status}). Rerun later.`);
        const retry = Number(response.headers.get('retry-after'));
        nextRequest = now() + Math.max(60_000, Math.min(300_000, Number.isFinite(retry) ? retry * 1000 : 60_000));
        continue;
      }
      if (!response.ok) throw new Error(`VirusTotal request failed (HTTP ${response.status}). Check the API key and quota.`);
      try { return await response.json(); }
      catch { throw new Error('VirusTotal returned invalid JSON.'); }
    }
  };
}

export async function scanInstaller(file, request, { wait = sleep, maxPolls = 20 } = {}) {
  const bytes = await readFile(file);
  if (!bytes.length || bytes.length > 650 * 1024 * 1024) throw new Error('Installer size is outside VirusTotal upload limits.');
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  let uploadUrl = `${API}/files`;
  if (bytes.length > 32 * 1024 * 1024) {
    const response = await request(`${API}/files/upload_url`);
    uploadUrl = response.data;
    if (typeof uploadUrl !== 'string') throw new Error('VirusTotal did not return an upload URL.');
  }
  const form = new FormData();
  form.append('file', new Blob([bytes]), path.basename(file));
  const uploaded = await request(uploadUrl, { method: 'POST', body: form });
  const id = uploaded.data?.id;
  if (typeof id !== 'string' || !id) throw new Error('VirusTotal did not return an analysis ID.');
  for (let poll = 0; poll < maxPolls; poll++) {
    await wait(30_000);
    const analysis = await request(`${API}/analyses/${encodeURIComponent(id)}`);
    const status = analysis.data?.attributes?.status;
    if (status === 'completed') {
      if (analysis.meta?.file_info?.sha256 !== sha256) throw new Error('VirusTotal analysis hash does not match the installer.');
      return { name: path.basename(file), sha256, url: `https://www.virustotal.com/gui/file/${sha256}` };
    }
    if (!['queued', 'in-progress'].includes(status)) throw new Error('VirusTotal returned an unexpected analysis status.');
  }
  throw new Error('VirusTotal analysis is still pending after 10 minutes. Rerun the workflow later.');
}

export function securityNotes(body, reports, repository) {
  const policy = `https://github.com/${repository}/blob/main/CODE_SIGNING.md`;
  const sections = [
    ['Безопасность', 'Политика подписи кода', 'Проверка на VirusTotal', 'SHA-256 установщика'],
    ['Security', 'Code signing policy', 'VirusTotal scan', 'Installer SHA-256'],
  ].map(([heading, signing, scan, hash]) => [
    `### ${heading}`, '', `- [${signing}](${policy})`,
    ...reports.map(report => `- [${scan} — ${report.kind}](${report.url})`), '',
    ...reports.map(report => `**${hash} (${report.kind}):** \`${report.sha256}\``),
  ].join('\n')).join('\n\n');
  const generated = `${START}\n${sections}\n${END}`;
  const existing = body || '';
  if (existing.includes(START) !== existing.includes(END)) throw new Error('Release security section markers are incomplete.');
  if (existing.includes(START)) {
    return existing.replace(/<!-- zapret-virustotal:start -->[\s\S]*?<!-- zapret-virustotal:end -->/, generated);
  }
  return `${existing.trimEnd()}${existing.trim() ? '\n\n' : ''}${generated}`;
}

export async function scanRelease({ github, context, core, apiKey, releaseId, version,
  bundleDirectory = 'src-tauri/target/release/bundle', clientOptions, scanOptions }) {
  const request = createClient(apiKey, clientOptions);
  const files = [];
  for (const [dir, kind, suffix] of [['nsis', 'EXE', `_${version}_x64-setup.exe`], ['msi', 'MSI', `_${version}_x64_en-US.msi`]]) {
    const directory = path.join(bundleDirectory, dir);
    const names = (await readdir(directory)).filter(name => name.endsWith(suffix));
    if (names.length !== 1) throw new Error(`Expected exactly one ${kind} installer for version ${version}; found ${names.length}.`);
    files.push({ file: path.join(directory, names[0]), kind });
  }
  const reports = [];
  for (const { file, kind } of files) {
    core.info(`Submitting ${kind} installer to VirusTotal and waiting for analysis.`);
    reports.push({ ...await scanInstaller(file, request, scanOptions), kind });
  }
  // Fetch immediately before editing so hand-written release notes survive reruns.
  const { data: release } = await github.rest.repos.getRelease({ ...context.repo, release_id: releaseId });
  for (const report of reports) {
    const asset = release.assets.find(asset => asset.name === report.name.replaceAll(' ', '.'));
    if (!asset || asset.digest !== `sha256:${report.sha256}`) {
      throw new Error(`Published ${report.kind} asset is missing or its SHA-256 does not match the scanned installer.`);
    }
  }
  await github.rest.repos.updateRelease({ ...context.repo, release_id: releaseId,
    body: securityNotes(release.body, reports, `${context.repo.owner}/${context.repo.repo}`) });
  core.info('Both VirusTotal analyses completed; bilingual security sections updated.');
}
