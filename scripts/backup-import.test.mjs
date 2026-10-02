import assert from 'node:assert/strict';
import test from 'node:test';

import { initUserLists, loadUserLists } from '../src/features/user-lists.js';
import { state } from '../src/lib/state.js';

const views = [
  ['site-include-list', 'site-include-search', 'list-general-user.txt', 'include'],
  ['site-exclude-list', 'site-exclude-search', 'list-exclude-user.txt', 'exclude'],
  ['ip-exclude-list', 'ip-exclude-search', 'ipset-exclude-user.txt', 'ips'],
];
const backup = {
  version: 1,
  include: ['one.example.com', 'two.example.com'],
  exclude: ['exclude.example.com', 'other.example.com'],
  ips: ['192.0.2.1', '198.51.100.0/24'],
};

class Element {
  children = [];
  value = '';
  textContent = '';
  classList = { add() {}, remove() {} };
  set innerHTML(value) { this.html = value; this.children = []; }
  get innerHTML() { return this.html ?? this.textContent; }
  appendChild(child) { this.children.push(child); }
  querySelector() { return new Element(); }
  addEventListener() {}
}

function setup(t, handler) {
  const elements = new Map();
  for (const [container, search] of views) {
    elements.set(container, new Element());
    elements.set(search, new Element());
  }
  for (const id of ['backup-import-btn', 'ip-backup-import-btn']) elements.set(id, new Element());
  const calls = [], alerts = [];
  const previous = { window: globalThis.window, document: globalThis.document };
  globalThis.document = {
    getElementById: id => elements.get(id) ?? null,
    createElement: () => new Element(),
  };
  globalThis.window = {
    confirm: () => true,
    alert: message => alerts.push(message),
    __TAURI__: { core: { invoke: async (command, args) => {
      calls.push({ command, args });
      if (command === 'get_zapret_status') return { running: true };
      return handler(command, args);
    } } },
  };
  state.pendingRestart = false;
  t.mock.method(globalThis, 'setTimeout', () => 0);
  t.mock.method(console, 'error', () => {});
  t.after(() => {
    globalThis.window = previous.window;
    globalThis.document = previous.document;
    state.pendingRestart = false;
  });
  initUserLists();
  return { elements, calls, alerts };
}

function assertDisplayed(elements, snapshot) {
  for (const [container, , , key] of views) {
    const rows = elements.get(container).children;
    assert.equal(rows.length, snapshot[key].length);
    snapshot[key].forEach((entry, index) => assert.ok(rows[index].innerHTML.includes(entry)));
  }
}

for (const button of ['backup-import-btn', 'ip-backup-import-btn']) {
  test(`${button} displays all restored lists immediately and clears searches`, async t => {
    const { elements, calls } = setup(t, command => {
      assert.equal(command, 'import_backup_file');
      return structuredClone(backup);
    });
    for (const [, search] of views) elements.get(search).value = 'does-not-match';
    await elements.get(button).onclick();
    assertDisplayed(elements, backup);
    for (const [, search] of views) assert.equal(elements.get(search).value, '');
    assert.deepEqual(calls.map(call => call.command), ['import_backup_file', 'get_zapret_status']);
    assert.equal(state.pendingRestart, true);
  });
}

test('a read started before import cannot overwrite the restored snapshot', async t => {
  const pending = [];
  const { elements } = setup(t, command => {
    if (command === 'import_backup_file') return structuredClone(backup);
    assert.equal(command, 'read_user_list');
    return new Promise(resolve => pending.push(resolve));
  });
  const staleLoad = loadUserLists();
  assert.equal(pending.length, 3);
  await elements.get('backup-import-btn').onclick();
  pending.forEach(resolve => resolve(['old.example.com']));
  assert.equal(await staleLoad, false);
  assertDisplayed(elements, backup);
});

test('cancelled import preserves visible lists and searches', async t => {
  const { elements, calls } = setup(t, (command, args) => {
    if (command === 'import_backup_file') return null;
    return backup[views.find(view => view[2] === args.filename)[3]];
  });
  await loadUserLists();
  elements.get('site-include-search').value = 'one';
  calls.length = 0;
  await elements.get('backup-import-btn').onclick();
  assertDisplayed(elements, backup);
  assert.equal(elements.get('site-include-search').value, 'one');
  assert.deepEqual(calls.map(call => call.command), ['import_backup_file']);
  assert.equal(state.pendingRestart, false);
});

test('failed import displays the error and does not mark success', async t => {
  const { elements, alerts } = setup(t, () => { throw new Error('Invalid list entry'); });
  await elements.get('backup-import-btn').onclick();
  assert.equal(alerts.length, 1);
  assert.ok(alerts[0].includes('Invalid list entry'));
  assert.equal(state.pendingRestart, false);
  assert.equal(elements.get('backup-import-btn').innerHTML, '');
});

test('one failed list read does not partially replace the displayed snapshot', async t => {
  const { elements } = setup(t, (command, args) => {
    if (command === 'import_backup_file') return structuredClone(backup);
    if (args.filename === 'ipset-exclude-user.txt') throw new Error('Read failed');
    return ['stale.example.com'];
  });
  await elements.get('backup-import-btn').onclick();
  assert.equal(await loadUserLists(), false);
  assertDisplayed(elements, backup);
});
