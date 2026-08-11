import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import en from '../src/i18n/en.js';
import ru from '../src/i18n/ru.js';

const sourceRoot = fileURLToPath(new URL('../src/', import.meta.url));

async function sourceFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const resolved = path.join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(resolved);
    return /\.(?:html|js)$/.test(entry.name) ? [resolved] : [];
  }));
  return nested.flat();
}

test('English and Russian dictionaries contain the same keys', () => {
  assert.deepEqual(Object.keys(ru).sort(), Object.keys(en).sort());
});

test('literal translation keys used by the UI exist in both dictionaries', async () => {
  const missing = [];
  for (const file of await sourceFiles(sourceRoot)) {
    const content = await readFile(file, 'utf8');
    const patterns = [
      /\bt\(\s*['"]([^'"]+)['"]/g,
      /\bdata-i18n\s*=\s*['"]([^'"]+)['"]/g,
    ];
    for (const pattern of patterns) {
      for (const match of content.matchAll(pattern)) {
        const key = match[1];
        if (!(key in ru) || !(key in en)) {
          missing.push(`${path.relative(sourceRoot, file)}: ${key}`);
        }
      }
    }
  }
  assert.deepEqual(missing, []);
});
