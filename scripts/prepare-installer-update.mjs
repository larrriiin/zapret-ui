import { readFile, writeFile, rename } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function prepareManifest(manifest, { version, signature, assetName, repository, tag }) {
  if (manifest.version?.replace(/^v/, '') !== version) throw new Error('Updater manifest version does not match the installer.');
  if (tag !== `v${version}` && tag !== version) throw new Error('Release tag does not match the installer version.');
  if (!/^[A-Za-z0-9][\w.-]*\/[A-Za-z0-9][\w.-]*$/.test(repository)) throw new Error('Invalid GitHub repository.');
  if (!signature?.trim() || !/^[A-Za-z0-9+/=\r\n]+$/.test(signature.trim())) throw new Error('Missing or invalid updater signature.');
  if (!assetName.endsWith('_x64-branded-setup.exe') || path.basename(assetName) !== assetName) throw new Error('Expected the branded x64 EXE.');
  if (!manifest.platforms || !Object.keys(manifest.platforms).some(key => /^windows-x86_64(?:-nsis)?$/.test(key))) {
    throw new Error('Manifest has no supported Windows EXE target.');
  }
  const update = {
    url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(assetName)}`,
    signature: signature.trim(),
  };
  return {
    ...manifest,
    platforms: {
      ...manifest.platforms,
      // Legacy clients request the generic key; newer clients prefer the NSIS-specific key.
      // Explicit MSI clients keep their original package and migration behavior.
      'windows-x86_64': update,
      'windows-x86_64-nsis': update,
    },
  };
}

async function main() {
  const [manifestPath, installerPath] = process.argv.slice(2);
  if (!manifestPath || !installerPath) throw new Error('Usage: node scripts/prepare-installer-update.mjs <latest.json> <branded.exe>');
  const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const manifest = JSON.parse((await readFile(manifestPath, 'utf8')).replace(/^\uFEFF/, ''));
  const signature = await readFile(`${installerPath}.sig`, 'utf8');
  const result = prepareManifest(manifest, {
    version: config.version,
    signature,
    assetName: path.basename(installerPath),
    repository: process.env.GITHUB_REPOSITORY,
    tag: process.env.GITHUB_REF_NAME,
  });
  const temporary = `${manifestPath}.tmp`;
  await writeFile(temporary, `${JSON.stringify(result, null, 2)}\n`);
  await rename(temporary, manifestPath);
  console.log(`Prepared signed branded updater for ZAPRET ${config.version}; preserved other platforms.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
