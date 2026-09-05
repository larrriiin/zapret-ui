import { readFile, writeFile, rename, readdir } from 'node:fs/promises';
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

// A release draft need not contain latest.json. Build it from this run's signed
// artifacts instead of downloading an asset that we are responsible for creating.
export async function createManifestFromBundle(bundleDirectory, options, { requireMsi = false } = {}) {
  const manifest = {
    version: options.version,
    notes: `ZAPRET ${options.version}`,
    pub_date: new Date().toISOString(),
    platforms: { 'windows-x86_64': {} },
  };
  // Validate release metadata before using it in paths or URLs.
  const result = prepareManifest(manifest, options);
  const msiDirectory = path.join(bundleDirectory, 'msi');
  let names;
  try { names = await readdir(msiDirectory); }
  catch (error) { if (error.code !== 'ENOENT') throw error; names = []; }
  const installers = names.filter(name => name.includes(`_${options.version}_x64`) && name.endsWith('.msi'));
  if (installers.length > 1) throw new Error('Multiple MSI packages found; cannot choose the updater package.');
  if (requireMsi && installers.length === 0) throw new Error('Configured MSI updater package is missing.');
  if (installers.length === 1) {
    const name = installers[0];
    const signature = (await readFile(path.join(msiDirectory, `${name}.sig`), 'utf8')).trim();
    if (!signature || !/^[A-Za-z0-9+/=\r\n]+$/.test(signature)) throw new Error('Missing or invalid MSI updater signature.');
    result.platforms['windows-x86_64-msi'] = {
      url: `https://github.com/${options.repository}/releases/download/${encodeURIComponent(options.tag)}/${encodeURIComponent(name)}`,
      signature,
    };
  }
  return result;
}

async function main() {
  const [manifestPath, installerPath, mode, bundleDirectory] = process.argv.slice(2);
  if (!manifestPath || !installerPath) throw new Error('Usage: node scripts/prepare-installer-update.mjs <latest.json> <branded.exe>');
  const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const signature = await readFile(`${installerPath}.sig`, 'utf8');
  const options = {
    version: config.version,
    signature,
    assetName: path.basename(installerPath),
    repository: process.env.GITHUB_REPOSITORY,
    tag: process.env.GITHUB_REF_NAME,
  };
  let result;
  if (mode === '--from-bundle' && bundleDirectory) {
    const targets = config.bundle.targets;
    result = await createManifestFromBundle(bundleDirectory, options, {
      requireMsi: targets === 'all' || targets === 'msi' || (Array.isArray(targets) && targets.includes('msi')),
    });
  } else if (mode) {
    throw new Error('Expected --from-bundle <bundle directory>.');
  } else {
    const manifest = JSON.parse((await readFile(manifestPath, 'utf8')).replace(/^\uFEFF/, ''));
    result = prepareManifest(manifest, options);
  }
  const temporary = `${manifestPath}.tmp`;
  await writeFile(temporary, `${JSON.stringify(result, null, 2)}\n`);
  await rename(temporary, manifestPath);
  console.log(`Prepared signed updater for ZAPRET ${config.version}: ${Object.keys(result.platforms).join(', ')}.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
