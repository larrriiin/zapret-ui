import { pathToFileURL } from 'node:url';

const STANDARD_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

export function normalizeVersion(tag) {
  if (typeof tag !== 'string') return null;
  const version = tag.startsWith('v') ? tag.slice(1) : tag;
  return STANDARD_VERSION.test(version) ? version : null;
}

export function compareVersions(left, right) {
  const normalizedLeft = normalizeVersion(left);
  const normalizedRight = normalizeVersion(right);
  if (!normalizedLeft || !normalizedRight) return null;

  const leftParts = normalizedLeft.split('.').map(BigInt);
  const rightParts = normalizedRight.split('.').map(BigInt);
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] > rightParts[index]) return 1;
    if (leftParts[index] < rightParts[index]) return -1;
  }
  return 0;
}

function comparisonName(comparison) {
  if (comparison === 1) return 'newer';
  if (comparison === -1) return 'older';
  return 'equal';
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [latestTag, currentVersion] = process.argv.slice(2);
  const latestVersion = normalizeVersion(latestTag);
  const comparison = compareVersions(latestTag, currentVersion);
  if (!latestVersion || comparison === null) {
    console.error(`Cannot safely compare Flowseal versions: latest=${JSON.stringify(latestTag)}, current=${JSON.stringify(currentVersion)}. Only canonical numeric x.y.z versions are automated.`);
    process.exitCode = 1;
  } else {
    console.log(`version=${latestVersion}`);
    console.log(`comparison=${comparisonName(comparison)}`);
  }
}
