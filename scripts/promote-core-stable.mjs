import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { rename, rm } from 'node:fs/promises';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import path from 'node:path';

const args = process.argv.slice(2);
let version;
const urls = [];
for (let i = 0; i < args.length; i += 2) {
  if (args[i] === '--version') version = args[i + 1];
  else if (args[i] === '--url') urls.push(args[i + 1]);
  else throw new Error(`Unknown argument: ${args[i]}`);
}
if (!version?.trim() || urls.length === 0) throw new Error('Usage: --version <version> --url <https-url> [--url <https-url>]');
for (const value of urls) if (new URL(value).protocol !== 'https:') throw new Error(`Only HTTPS artifacts are allowed: ${value}`);

const artifacts = [];
for (const url of urls) {
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok || !response.body) throw new Error(`Download failed (${response.status}): ${url}`);
  const hash = createHash('sha256');
  await pipeline(Readable.fromWeb(response.body), new Transform({ transform(chunk, _encoding, callback) { hash.update(chunk); callback(); } }));
  artifacts.push({ url, checksum: { algorithm: 'sha256', value: hash.digest('hex') } });
}
const target = path.resolve('core-channel/stable.json');
const temporary = `${target}.tmp-${process.pid}`;
try {
  await pipeline(Readable.from(`${JSON.stringify({ schemaVersion: 1, channel: 'stable', provider: 'flowseal', version, artifacts }, null, 2)}\n`), createWriteStream(temporary, { flags: 'wx' }));
  await rename(temporary, target);
  // Never leave a detached signature next to content it no longer signs.
  await rm(`${target}.sig`, { force: true });
} finally { await rm(temporary, { force: true }); }
console.log(`Promoted Flowseal ${version} with ${artifacts.length} verified artifact(s). Sign stable.json before committing.`);
