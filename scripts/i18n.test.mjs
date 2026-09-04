import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import en from '../src/i18n/en.js';
import ru from '../src/i18n/ru.js';
import {
  DIAGNOSTIC_MESSAGE_KEYS,
  DIAGNOSTIC_NAME_KEYS,
} from '../src/features/diagnostics.js';

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

test('every static backend diagnostic label and message has a frontend localization', async () => {
  const rust = await readFile(fileURLToPath(new URL('../src-tauri/src/lib.rs', import.meta.url)), 'utf8');
  const start = rust.indexOf('async fn run_diagnostics()');
  const end = rust.indexOf('\n#[derive(serde::Serialize)]', start);
  assert.notEqual(start, -1);
  assert.notEqual(end, -1);
  const diagnosticsSource = rust.slice(start, end);

  const names = [...diagnosticsSource.matchAll(/name:\s*"([^"]+)"\.to_string\(\)/g)].map((match) => match[1]);
  const messages = [...diagnosticsSource.matchAll(/message:\s*"([^"]+)"\s*\.to_string\(\)/g)].map((match) => match[1]);
  const missingNames = [...new Set(names)].filter((name) => !(name in DIAGNOSTIC_NAME_KEYS));
  const missingMessages = [...new Set(messages)].filter((message) => !(message in DIAGNOSTIC_MESSAGE_KEYS));
  const mappedTranslationKeys = [
    ...Object.values(DIAGNOSTIC_NAME_KEYS),
    ...Object.values(DIAGNOSTIC_MESSAGE_KEYS),
  ];
  const missingTranslations = mappedTranslationKeys.filter((key) => !(key in ru) || !(key in en));

  assert.deepEqual(missingNames, []);
  assert.deepEqual(missingMessages, []);
  assert.deepEqual(missingTranslations, []);
});
