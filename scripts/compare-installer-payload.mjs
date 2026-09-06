// Static comparison only: never load or execute either PE file.
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
const [wrapperPath, payloadPath] = process.argv.slice(2);
if (!wrapperPath || !payloadPath) throw new Error('Usage: node scripts/compare-installer-payload.mjs <wrapper.exe> <nsis.exe>');
const [wrapper, payload] = await Promise.all([readFile(wrapperPath), readFile(payloadPath)]);
if (!payload.length) throw new Error('Payload is empty.');
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const offset = wrapper.indexOf(payload);
console.log(JSON.stringify({ wrapperSha256: sha256(wrapper), payloadSha256: sha256(payload), payloadBytes: payload.length, offset, identicalPayload: offset >= 0 }, null, 2));
if (offset < 0) process.exitCode = 1;
