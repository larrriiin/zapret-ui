import assert from 'node:assert/strict';
import test from 'node:test';
import { adminReason } from '../src/lib/privilege-errors.js';

test('permission reasons survive IPC and background error context', () => {
  assert.equal(adminReason('admin_required:service'), 'service');
  assert.equal(adminReason('WARP: admin_required: warp_apps'), 'warp_apps');
  assert.equal(adminReason({ code: 'admin_required', detail: 'traffic' }), 'traffic');
  assert.equal(adminReason({ code: 'warp_apps_admin' }), 'warp_apps');
  for (const error of [null, 'Access denied', 'admin_required:arbitrary', { code: 'warp_process' }]) assert.equal(adminReason(error), null);
});
