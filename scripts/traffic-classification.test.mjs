import assert from 'node:assert/strict';
import test from 'node:test';
import { groupTrafficConnections, matchesTrafficMode, trafficClassificationLabels } from '../src/features/traffic.js';

test('route and Zapret processing are independent filters', () => {
  const directExpected = { route: 'direct', zapret: { state: 'expected', profile: 2, reason: 'hostlist' } };
  assert.equal(matchesTrafficMode(directExpected, 'direct'), true);
  assert.equal(matchesTrafficMode(directExpected, 'zapret'), true);
  const proxy = { route: 'tg_proxy_warp', zapret: { state: 'unknown', reason: 'proxy' } };
  for (const mode of ['warp', 'tg_proxy', 'unknown', 'all']) assert.equal(matchesTrafficMode(proxy, mode), true);
  assert.equal(matchesTrafficMode(proxy, 'zapret'), false);
  assert.equal(matchesTrafficMode(proxy, 'direct'), false);
});

test('unknown evidence is never silently presented as Direct or expected processing', () => {
  const partial = { route: 'unknown', zapret: { state: 'unknown', reason: 'waiting_hostname' } };
  assert.equal(matchesTrafficMode(partial, 'unknown'), true);
  assert.equal(matchesTrafficMode(partial, 'direct'), false);
  assert.equal(matchesTrafficMode(partial, 'zapret'), false);
  const labels = trafficClassificationLabels(partial);
  assert.match(labels.route, /Неизвестно/);
  assert.match(labels.zapret, /неизвестно/);
  assert.ok(labels.reason.length > 0);
});

test('local traffic is hidden by default without hiding known proxy loopback connections', () => {
  assert.equal(matchesTrafficMode({ route: 'local' }, 'all'), false);
  assert.equal(matchesTrafficMode({ route: 'local' }, 'all', false), true);
  assert.equal(matchesTrafficMode({ route: 'tg_proxy' }, 'all'), true);
});

test('an exclusion has an explanation and is not an expected Zapret match', () => {
  const excluded = { route: 'direct', zapret: { state: 'excluded', reason: 'excluded' } };
  assert.equal(matchesTrafficMode(excluded, 'zapret'), false);
  assert.equal(matchesTrafficMode(excluded, 'direct'), true);
  assert.match(trafficClassificationLabels(excluded).zapret, /исключено/);
});

test('one process and IP combine across PIDs, ports and protocols without losing classifications', () => {
  const connections = [
    { process_name: 'Browser.exe', pid: 10, remote_address: '2001:db8::1', remote_port: 443, protocol: 'TCP', hostname: 'a.example', state: 'established', route: 'direct', zapret: { state: 'expected', reason: 'hostlist', profile: 1 }, download_bps: 100, upload_bps: 20, download_bytes: 300, upload_bytes: 40, zapret_download_bps: 100, zapret_upload_bps: 20, zapret_download_bytes: 300, zapret_upload_bytes: 40 },
    { process_name: 'browser.EXE', pid: 20, remote_address: '2001:db8::1', remote_port: 80, protocol: 'UDP', hostname: 'b.example', state: 'observed', route: 'warp', zapret: { state: 'unknown', reason: 'proxy' }, download_bps: 200, upload_bps: 30, download_bytes: 500, upload_bytes: 60 },
    { process_name: 'Browser.exe', pid: 10, remote_address: '2001:db8::1', remote_port: 443, protocol: 'TCP', hostname: 'a.example', state: 'established' },
  ];
  const original = structuredClone(connections);
  const [group] = groupTrafficConnections(connections);
  assert.equal(groupTrafficConnections(connections).length, 1);
  assert.equal(group.connections.length, 3);
  assert.deepEqual(group.pids, [10, 20]);
  assert.deepEqual(group.ports, [80, 443]);
  assert.deepEqual(group.protocols, ['TCP', 'UDP']);
  assert.deepEqual(group.hostnames, ['a.example', 'b.example']);
  assert.deepEqual(group.states, ['established', 'observed']);
  for (const [field, total] of Object.entries({ download_bps: 300, upload_bps: 50, download_bytes: 800, upload_bytes: 100, zapret_download_bps: 100, zapret_upload_bps: 20, zapret_download_bytes: 300, zapret_upload_bytes: 40 })) assert.equal(group[field], total, field);
  assert.equal(group.connections[0].zapret.profile, 1);
  assert.equal(group.connections[1].route, 'warp');
  assert.deepEqual(connections, original);
});

test('different processes or IPs stay separate and empty input has no groups', () => {
  assert.deepEqual(groupTrafficConnections([]), []);
  const connections = [
    { process_name: 'a.exe', remote_address: '192.0.2.1' },
    { process_name: 'b.exe', remote_address: '192.0.2.1' },
    { process_name: 'a.exe', remote_address: '192.0.2.2' },
    { pid: 10, remote_address: '192.0.2.1' },
    { pid: 20, remote_address: '192.0.2.1' },
  ];
  assert.equal(groupTrafficConnections(connections).length, 5);
});

test('filtering before grouping preserves only matching counters and classifications', () => {
  const connections = [
    { process_name: 'a.exe', remote_address: '192.0.2.1', route: 'direct', zapret: { state: 'expected' }, download_bps: 50, zapret_download_bps: 50 },
    { process_name: 'a.exe', remote_address: '192.0.2.1', route: 'warp', zapret: { state: 'unknown' }, download_bps: 100 },
  ];
  for (const [mode, route, rate] of [['warp', 'warp', 100], ['zapret', 'direct', 50]]) {
    const [group] = groupTrafficConnections(connections.filter(connection => matchesTrafficMode(connection, mode)));
    assert.equal(group.connections.length, 1);
    assert.equal(group.connections[0].route, route);
    assert.equal(group.download_bps, rate);
  }
});
