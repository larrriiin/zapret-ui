import test from 'node:test';
import assert from 'node:assert/strict';
import { connectionLabel } from '../src/features/connection-summary.js';

test('connection summary covers all independent provider combinations', () => {
  assert.equal(connectionLabel({running:false}, false, 'Connected'), '');
  assert.equal(connectionLabel({running:false}, true, 'Connected'), 'Cloudflare');
  assert.equal(connectionLabel({running:true, strategy:'general (ALT12)'}, false, 'Connected'), 'general (ALT12)');
  assert.equal(connectionLabel({running:true, strategy:'general (ALT12)'}, true, 'Connected'), 'general (ALT12) + Cloudflare');
  assert.equal(connectionLabel({running:true}, true, 'Подключено'), 'Подключено + Cloudflare');
  assert.equal(connectionLabel({running:false}, false, 'Connected', true), 'WireSock');
  assert.equal(connectionLabel({running:true, strategy:'general (ALT12)'}, false, 'Connected', true), 'general (ALT12) + WireSock');
});
