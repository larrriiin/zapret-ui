import test from 'node:test';
import assert from 'node:assert/strict';
import { routingState, needsProxyConfirmation, orderedWarpModes } from '../src/features/warp-state.js';

test('desired selection never reports an active runtime after restart or failure', () => {
  assert.equal(routingState({ enabled: false, runtime_active: false }), 'off');
  assert.equal(routingState({ enabled: true, runtime_active: false }), 'configured');
  assert.equal(routingState({ enabled: true, runtime_active: true }), 'active');
  assert.equal(routingState({ enabled: false }, { starting: true }), 'starting');
  assert.equal(routingState({ enabled: true, runtime_active: false }, { error: { code: 'warp_cli_error' } }), 'error');
  assert.equal(routingState({ enabled: true, error: 'driver failed' }), 'error');
});
test('sites and applications have independent execution states', () => {
  const sites = { enabled: false, runtime_active: false };
  const applications = { enabled: true, runtime_active: true };
  assert.equal(routingState(sites), 'off'); assert.equal(routingState(applications), 'active');
});
test('only full-device tunnel modes request confirmation before local proxy activation', () => {
  for (const mode of ['warp', 'warp+dot', 'warp+doh', 'tunnel_only']) assert.equal(needsProxyConfirmation({ mode }), true);
  for (const mode of ['doh', 'dot', 'proxy', null]) assert.equal(needsProxyConfirmation({ mode }), false);
});
test('traffic and DNS comes first, local proxy second, remaining modes keep their order', () => {
  const modes = ['doh', 'dot', 'warp', 'warp+dot', 'warp+doh', 'proxy', 'tunnel_only'];
  assert.deepEqual(orderedWarpModes(modes), ['warp', 'proxy', 'doh', 'dot', 'warp+dot', 'warp+doh', 'tunnel_only']);
  assert.deepEqual(modes, ['doh', 'dot', 'warp', 'warp+dot', 'warp+doh', 'proxy', 'tunnel_only']);
  assert.deepEqual(orderedWarpModes(['dot', 'proxy', 'doh']), ['proxy', 'dot', 'doh']);
  assert.deepEqual(orderedWarpModes([]), []);
});
