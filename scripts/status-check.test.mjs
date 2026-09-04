import assert from 'node:assert/strict';
import test from 'node:test';

import { getSystemSummaryState } from '../src/features/status-check.js';

test('temporary launch is reported as running even when the Windows service is stopped', () => {
  assert.equal(getSystemSummaryState({
    running: true,
    strategy: 'general (FAKE TLS AUTO ALT3)',
    zapret_service: 'stopped',
    windivert_service: 'running',
    bypass_process: 'running',
  }), 'running');
});

test('stopped process is reported as stopped', () => {
  assert.equal(getSystemSummaryState({
    running: false,
    strategy: null,
    zapret_service: 'stopped',
    windivert_service: 'stopped',
    bypass_process: 'stopped',
  }), 'stopped');
});

test('running process with incomplete component data requires attention', () => {
  assert.equal(getSystemSummaryState({
    running: true,
    strategy: null,
    zapret_service: 'stopped',
    windivert_service: 'running',
    bypass_process: 'running',
  }), 'attention');
});
