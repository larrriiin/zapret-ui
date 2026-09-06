import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const hooks = await readFile(new URL('../installer/branding.nsh', import.meta.url), 'utf8');

test('renaming the product preserves the legacy Windows installation identity', () => {
  assert.equal(config.productName, 'ZAPRET UI');
  assert.equal(config.bundle.windows.wix.upgradeCode, '81d7cfed-6c74-57ea-84fb-1d129cb455c4');
  assert.match(hooks, /!macro NSIS_HOOK_PREINSTALL/);
  assert.match(hooks, /Uninstall\\ZAPRET/);
  assert.match(hooks, /StrCpy \$INSTDIR "\$R0"/);
  assert.match(hooks, /!macro NSIS_HOOK_POSTINSTALL/);
});
