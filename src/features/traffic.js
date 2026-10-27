import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { showSection } from './navigation.js';

const monitor = {
  running: false,
  busy: false,
  mode: 'all',
  snapshot: null,
  timer: null,
  history: [],
  focus: null,
  tab: 'connections',
  hideSystem: true,
  hideLocal: true,
};

const HISTORY_LENGTH = 90;

const ROUTE_KEYS = { direct: 'traffic_route_direct', warp: 'traffic_route_warp', tg_proxy: 'traffic_route_tg', tg_proxy_warp: 'traffic_route_tg_warp', local: 'traffic_route_local', unknown: 'traffic_route_unknown' };
const ZAPRET_KEYS = { expected: 'traffic_zapret_expected', excluded: 'traffic_zapret_excluded', not_matched: 'traffic_zapret_not_matched', unknown: 'traffic_zapret_unknown', off: 'traffic_zapret_off' };
const REASON_KEYS = { ports: 'traffic_reason_ports', ipset: 'traffic_reason_ipset', hostlist: 'traffic_reason_hostlist', l7: 'traffic_reason_l7', global: 'traffic_reason_global', excluded: 'traffic_reason_excluded', waiting_hostname: 'traffic_reason_hostname', waiting_protocol: 'traffic_reason_protocol', unsupported: 'traffic_reason_unsupported', no_profile: 'traffic_reason_no_profile', off: 'traffic_zapret_off', rules_unknown: 'traffic_reason_rules', no_action: 'traffic_reason_no_action', proxy: 'traffic_reason_proxy', autohostlist: 'traffic_reason_auto' };

export function matchesTrafficMode(connection, mode, hideLocal = true) {
  if (hideLocal && connection.route === 'local') return false;
  if (mode === 'zapret') return connection.zapret?.state === 'expected';
  if (mode === 'warp') return ['warp', 'tg_proxy_warp'].includes(connection.route);
  if (mode === 'tg_proxy') return ['tg_proxy', 'tg_proxy_warp'].includes(connection.route);
  if (mode === 'direct') return connection.route === 'direct';
  if (mode === 'unknown') return connection.route === 'unknown' || connection.zapret?.state === 'unknown';
  return true;
}

export function trafficClassificationLabels(connection) {
  const zapret = connection.zapret || { state: 'unknown', reason: 'rules_unknown' };
  const reason = t(REASON_KEYS[zapret.reason] || 'traffic_reason_rules');
  return { route: t(ROUTE_KEYS[connection.route] || 'traffic_route_unknown'), zapret: t(ZAPRET_KEYS[zapret.state] || 'traffic_zapret_unknown'), reason, profile: zapret.profile ? t('traffic_profile_number', { number: zapret.profile }) : '' };
}

function classificationBadges(connection) {
  const labels = trafficClassificationLabels(connection);
  const detail = [labels.profile, labels.reason].filter(Boolean).join(' · ');
  return `<span class="traffic-route" data-route="${escapeHtml(connection.route || 'unknown')}" title="${escapeHtml(t('traffic_route_label'))}">${escapeHtml(labels.route)}</span><span class="traffic-zapret" data-state="${escapeHtml(connection.zapret?.state || 'unknown')}" title="${escapeHtml(detail)}">${escapeHtml(labels.zapret)}</span>`;
}

const TRAFFIC_COUNTERS = ['download_bps', 'upload_bps', 'download_bytes', 'upload_bytes', 'zapret_download_bps', 'zapret_upload_bps', 'zapret_download_bytes', 'zapret_upload_bytes'];

export function groupTrafficConnections(connections) {
  const groups = new Map();
  for (const connection of connections) {
    const processName = connection.process_name || '';
    const key = JSON.stringify([processName.toLowerCase(), processName ? null : connection.pid, connection.remote_address]);
    let group = groups.get(key);
    if (!group) {
      group = { process_name: processName, remote_address: connection.remote_address, connections: [], pids: new Set(), ports: new Set(), protocols: new Set(), hostnames: new Set(), states: new Set() };
      for (const field of TRAFFIC_COUNTERS) group[field] = 0;
      groups.set(key, group);
    }
    group.connections.push(connection);
    group.pids.add(connection.pid || '—');
    group.ports.add(connection.remote_port);
    group.protocols.add(connection.protocol);
    if (connection.hostname) group.hostnames.add(connection.hostname);
    if (connection.state) group.states.add(connection.state);
    for (const field of TRAFFIC_COUNTERS) group[field] += Number(connection[field]) || 0;
  }
  return Array.from(groups.values(), (group) => ({
    ...group,
    pids: [...group.pids].sort((a, b) => a - b),
    ports: [...group.ports].sort((a, b) => a - b),
    protocols: [...group.protocols].sort(),
    hostnames: [...group.hostnames].sort(),
    states: [...group.states].sort(),
  }));
}

function groupClassificationBadges(group) {
  const routes = new Map();
  const states = new Map();
  for (const connection of group.connections) {
    const labels = trafficClassificationLabels(connection);
    const route = connection.route || 'unknown';
    const state = connection.zapret?.state || 'unknown';
    if (!routes.has(route)) routes.set(route, { label: labels.route, count: 0 });
    routes.get(route).count += 1;
    if (!states.has(state)) states.set(state, { label: labels.zapret, count: 0, details: new Set() });
    const status = states.get(state);
    status.count += 1;
    status.details.add([labels.profile, labels.reason].filter(Boolean).join(' · '));
  }
  const countTitle = (count) => t('traffic_connection_group_count', { count });
  const routeBadges = [...routes].map(([route, { label, count }]) => `<span class="traffic-route" data-route="${escapeHtml(route)}" title="${escapeHtml(`${t('traffic_route_label')} · ${countTitle(count)}`)}">${escapeHtml(label)}</span>`);
  const stateBadges = [...states].map(([state, { label, count, details }]) => `<span class="traffic-zapret" data-state="${escapeHtml(state)}" title="${escapeHtml([countTitle(count), ...details].join('\n'))}">${escapeHtml(label)}</span>`);
  return [...routeBadges, ...stateBadges].join('');
}

function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');
}

function formatBytes(value, suffix = '') {
  const bytes = Math.max(0, Number(value) || 0);
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let amount = bytes;
  let index = 0;
  while (amount >= 1024 && index < units.length - 1) {
    amount /= 1024;
    index += 1;
  }
  const digits = index === 0 ? 0 : amount >= 100 ? 0 : amount >= 10 ? 1 : 2;
  return `${amount.toFixed(digits)} ${units[index]}${suffix}`;
}

function selectedMetrics(snapshot = monitor.snapshot) {
  if (!snapshot) return { uploadBps: 0, downloadBps: 0, uploadBytes: 0, downloadBytes: 0 };
  if (monitor.focus || !['all', 'zapret'].includes(monitor.mode)) {
    return (snapshot.connections || [])
      .filter(connectionMatchesFocus)
      .filter((connection) => matchesTrafficMode(connection, monitor.mode, monitor.hideLocal))
      .reduce((total, connection) => {
        const prefix = monitor.mode === 'zapret' ? 'zapret_' : '';
        total.uploadBps += connection[`${prefix}upload_bps`] || 0;
        total.downloadBps += connection[`${prefix}download_bps`] || 0;
        total.uploadBytes += connection[`${prefix}upload_bytes`] || 0;
        total.downloadBytes += connection[`${prefix}download_bytes`] || 0;
        return total;
      }, { uploadBps: 0, downloadBps: 0, uploadBytes: 0, downloadBytes: 0 });
  }
  if (monitor.mode === 'zapret') {
    return {
      uploadBps: snapshot.zapret_upload_bps,
      downloadBps: snapshot.zapret_download_bps,
      uploadBytes: snapshot.zapret_upload_bytes,
      downloadBytes: snapshot.zapret_download_bytes,
    };
  }
  return {
    uploadBps: snapshot.upload_bps,
    downloadBps: snapshot.download_bps,
    uploadBytes: snapshot.upload_bytes,
    downloadBytes: snapshot.download_bytes,
  };
}

function connectionMatchesFocus(connection) {
  if (!monitor.focus) return true;
  if (monitor.focus.type === 'process') return connection.process_name.toLowerCase() === monitor.focus.value;
  return connection.remote_address === monitor.focus.value;
}

function eventMatchesFocus(event) {
  if (!monitor.focus) return true;
  if (monitor.focus.type === 'process') return event.process_name.toLowerCase() === monitor.focus.value;
  return event.remote_address === monitor.focus.value;
}

function filteredConnections() {
  const query = ($('traffic-search')?.value || '').trim().toLowerCase();
  return (monitor.snapshot?.connections || []).filter((connection) => {
    if (!matchesTrafficMode(connection, monitor.mode, monitor.hideLocal)) return false;
    if (monitor.hideSystem && connection.system_process) return false;
    if (!connectionMatchesFocus(connection)) return false;
    if (!query) return true;
    return [
      connection.process_name,
      connection.pid,
      connection.remote_address,
      connection.remote_port,
      connection.local_address,
      connection.local_port,
      connection.protocol,
      connection.hostname,
      connection.route,
    ].some((value) => String(value).toLowerCase().includes(query));
  });
}

function setStatus(snapshot) {
  const dot = $('traffic-status-dot');
  const text = $('traffic-status-text');
  const strategy = $('traffic-strategy');
  const toggleText = $('traffic-toggle-text');
  const toggleIcon = $('traffic-toggle-icon');
  const toggle = $('traffic-toggle-btn');

  if (snapshot?.error) {
    dot.className = 'w-2 h-2 rounded-full bg-error-dim shadow-[0_0_8px_rgba(215,51,87,.7)]';
    text.textContent = `${t('traffic_error')}: ${snapshot.error}`;
    text.className = 'text-[10px] font-bold text-error-dim';
  } else if (monitor.running) {
    dot.className = 'w-2 h-2 rounded-full bg-secondary shadow-[0_0_8px_rgba(83,221,252,.7)]';
    text.textContent = snapshot?.filter_active
      ? t('traffic_running_zapret')
      : t('traffic_running_no_filter');
    text.className = 'text-[10px] font-bold uppercase tracking-widest text-secondary/80';
  } else {
    dot.className = 'w-2 h-2 rounded-full bg-on-surface-variant/30';
    text.textContent = t('traffic_stopped');
    text.className = 'text-[10px] font-bold uppercase tracking-widest text-on-surface-variant/60';
  }

  if (strategy && snapshot?.strategy) {
    strategy.textContent = `${t('traffic_strategy')}: ${snapshot.strategy}`;
    strategy.classList.remove('hidden');
  } else {
    strategy?.classList.add('hidden');
  }

  if (toggleText) toggleText.textContent = t(monitor.running ? 'traffic_stop' : 'traffic_start');
  if (toggleIcon) toggleIcon.textContent = monitor.running ? 'stop' : 'play_arrow';
  toggle?.classList.toggle('text-error-dim', monitor.running);
  toggle?.classList.toggle('border-error-dim/25', monitor.running);
  toggle?.classList.toggle('bg-error-dim/10', monitor.running);
  toggle?.classList.toggle('text-secondary', !monitor.running);
  toggle?.classList.toggle('border-secondary/25', !monitor.running);
  toggle?.classList.toggle('bg-secondary/10', !monitor.running);
  if (toggle) toggle.disabled = monitor.busy;
}

function renderConnections() {
  const list = $('traffic-connections-list');
  const empty = $('traffic-connections-empty');
  if (!list || !empty) return;
  const connections = filteredConnections();
  $('traffic-connection-count').textContent = String(connections.length);
  empty.classList.toggle('hidden', connections.length > 0);

  list.innerHTML = groupTrafficConnections(connections).map((connection) => {
    const downloadBps = monitor.mode === 'zapret' ? connection.zapret_download_bps : connection.download_bps;
    const uploadBps = monitor.mode === 'zapret' ? connection.zapret_upload_bps : connection.upload_bps;
    const downloadBytes = monitor.mode === 'zapret' ? connection.zapret_download_bytes : connection.download_bytes;
    const uploadBytes = monitor.mode === 'zapret' ? connection.zapret_upload_bytes : connection.upload_bytes;
    const address = connection.remote_address.includes(':') ? `[${connection.remote_address}]` : connection.remote_address;
    const endpoint = connection.ports.length === 1 ? `${address}:${connection.ports[0]}` : address;
    const count = t('traffic_connection_group_count', { count: connection.connections.length });
    const processDetail = `PID ${connection.pids.join(', ')} · ${connection.states.join(', ')}`;
    const endpointDetail = [t('traffic_track_ip'), endpoint, t('traffic_group_ports', { ports: connection.ports.join(', ') }), ...connection.hostnames].join('\n');
    return `<div class="traffic-connection-grid grid gap-3 items-center px-4 py-3 hover:bg-primary/[0.035] transition-colors text-[11px]">
      <div class="min-w-0">
        <div class="flex items-center gap-2 min-w-0">
          <button type="button" data-focus-type="process" data-focus-value="${escapeHtml(connection.process_name.toLowerCase())}" data-focus-label="${escapeHtml(connection.process_name)}" class="truncate font-semibold text-on-surface hover:text-tertiary transition-colors" title="${escapeHtml(t('traffic_track_process'))}">${escapeHtml(connection.process_name)}</button>
        </div>
        <span class="block truncate text-[9px] text-on-surface-variant/55" title="${escapeHtml(processDetail)}">${escapeHtml(count)}</span>
        <div class="traffic-classification">${groupClassificationBadges(connection)}</div>
      </div>
      <button type="button" data-focus-type="ip" data-focus-value="${escapeHtml(connection.remote_address)}" data-focus-label="${escapeHtml(connection.remote_address)}" class="min-w-0 text-left font-mono text-[10px] text-on-surface-variant/75 hover:text-tertiary truncate transition-colors" title="${escapeHtml(endpointDetail)}">${connection.hostnames.length ? `<span class="block truncate text-on-surface">${escapeHtml(connection.hostnames[0])}${connection.hostnames.length > 1 ? ` (+${connection.hostnames.length - 1})` : ''}</span>` : ''}${escapeHtml(endpoint)}${connection.ports.length > 1 ? `<span class="block truncate text-[9px] text-on-surface-variant/55">${escapeHtml(t('traffic_group_ports', { ports: connection.ports.join(', ') }))}</span>` : ''}</button>
      <span class="text-[9px] font-black text-on-surface-variant/55">${escapeHtml(connection.protocols.join('/'))}</span>
      <span class="text-right font-mono text-secondary">${formatBytes(downloadBps, '/s')}</span>
      <span class="text-right font-mono text-primary">${formatBytes(uploadBps, '/s')}</span>
      <span class="text-right font-mono text-on-surface-variant/55">${formatBytes(downloadBytes + uploadBytes)}</span>
    </div>`;
  }).join('');
}

function filteredEvents() {
  const query = ($('traffic-search')?.value || '').trim().toLowerCase();
  return (monitor.snapshot?.events || []).filter((event) => {
    if (!matchesTrafficMode(event, monitor.mode, monitor.hideLocal)) return false;
    if (monitor.hideSystem && event.system_process) return false;
    if (!eventMatchesFocus(event)) return false;
    if (!query) return true;
    return [event.process_name, event.pid, event.remote_address, event.remote_port, event.protocol]
      .some((value) => String(value).toLowerCase().includes(query));
  });
}

function renderLog() {
  const list = $('traffic-log-list');
  const empty = $('traffic-log-empty');
  if (!list || !empty) return;
  const events = filteredEvents();
  empty.classList.toggle('hidden', events.length > 0);
  const eventLabels = {
    connecting: 'traffic_event_connecting',
    connected: 'traffic_event_connected',
    closed: 'traffic_event_closed',
    reset: 'traffic_event_reset',
    udp: 'traffic_event_udp',
    observed: 'traffic_event_observed',
    classified: 'traffic_event_classified',
  };
  list.innerHTML = events.map((event) => {
    const endpoint = event.remote_address.includes(':')
      ? `[${event.remote_address}]:${event.remote_port}`
      : `${event.remote_address}:${event.remote_port}`;
    const success = event.event_type === 'connected';
    const failed = event.event_type === 'reset';
    const resultClass = success ? 'text-secondary' : failed ? 'text-error-dim' : 'text-on-surface-variant/60';
    const icon = success ? 'check_circle' : failed ? 'error' : event.event_type === 'connecting' ? 'sync' : 'info';
    return `<div class="grid grid-cols-[86px_minmax(140px,1fr)_minmax(180px,1.5fr)_110px] gap-3 items-center px-4 py-3 text-[10px] hover:bg-primary/[0.035]">
      <span class="font-mono text-on-surface-variant/45">${new Date(event.timestamp_ms).toLocaleTimeString()}</span>
      <div class="min-w-0"><button type="button" data-focus-type="process" data-focus-value="${escapeHtml(event.process_name.toLowerCase())}" data-focus-label="${escapeHtml(event.process_name)}" class="text-left truncate text-on-surface hover:text-tertiary">${escapeHtml(event.process_name)} <span class="text-on-surface-variant/35">· ${event.pid || '—'}</span></button><div class="traffic-classification">${classificationBadges(event)}</div></div>
      <button type="button" data-focus-type="ip" data-focus-value="${escapeHtml(event.remote_address)}" data-focus-label="${escapeHtml(event.remote_address)}" class="text-left truncate font-mono text-on-surface-variant/70 hover:text-tertiary" title="${escapeHtml(endpoint)}">${escapeHtml(endpoint)}</button>
      <span class="flex items-center gap-1.5 ${resultClass}"><span class="material-symbols-outlined text-sm">${icon}</span>${escapeHtml(t(eventLabels[event.event_type] || 'traffic_event_observed'))}</span>
    </div>`;
  }).join('');
}

function renderFocus() {
  const bar = $('traffic-focus-bar');
  if (!bar) return;
  bar.classList.toggle('hidden', !monitor.focus);
  bar.classList.toggle('flex', Boolean(monitor.focus));
  if (monitor.focus) {
    $('traffic-focus-label').textContent = `${t(monitor.focus.type === 'process' ? 'traffic_process' : 'traffic_remote')}: ${monitor.focus.label}`;
  }
}

function setFocus(type, value, label) {
  monitor.focus = { type, value: String(value), label };
  monitor.history = [];
  if (monitor.snapshot) addHistoryPoint();
  render();
}

function clearFocus() {
  monitor.focus = null;
  monitor.history = [];
  if (monitor.snapshot) addHistoryPoint();
  render();
}

function setTab(tab) {
  monitor.tab = tab;
  $('traffic-connections-panel')?.classList.toggle('hidden', tab !== 'connections');
  $('traffic-log-panel')?.classList.toggle('hidden', tab !== 'log');
  document.querySelectorAll('.traffic-tab-btn').forEach((button) => {
    const active = button.dataset.tab === tab;
    button.classList.toggle('bg-primary/10', active);
    button.classList.toggle('text-primary', active);
    button.classList.toggle('text-on-surface-variant/45', !active);
  });
}

function renderMetrics() {
  const metrics = selectedMetrics();
  $('traffic-download-rate').textContent = formatBytes(metrics.downloadBps, '/s');
  $('traffic-upload-rate').textContent = formatBytes(metrics.uploadBps, '/s');
  $('traffic-download-total').textContent = formatBytes(metrics.downloadBytes);
  $('traffic-upload-total').textContent = formatBytes(metrics.uploadBytes);
  $('traffic-candidate-note').textContent = t(monitor.mode === 'zapret' ? 'traffic_candidates_note' : 'traffic_all_note');
}

function drawChart() {
  const canvas = $('traffic-chart');
  if (!canvas) return;
  const rect = canvas.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return;
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(rect.width * ratio);
  canvas.height = Math.round(rect.height * ratio);
  const ctx = canvas.getContext('2d');
  ctx.scale(ratio, ratio);
  const width = rect.width;
  const height = rect.height;
  ctx.clearRect(0, 0, width, height);

  const palette = getComputedStyle(document.documentElement);
  const color = (name) => palette.getPropertyValue(`--color-${name}`).trim();
  ctx.strokeStyle = color('outline-variant');
  ctx.globalAlpha = 0.25;
  ctx.lineWidth = 1;
  for (let row = 1; row < 4; row += 1) {
    const y = (height / 4) * row;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }

  ctx.globalAlpha = 1;
  const values = monitor.history.flatMap((point) => [point.download, point.upload]);
  const max = Math.max(1024, ...values) * 1.12;
  const plot = (key, stroke, fill) => {
    if (monitor.history.length < 2) return;
    const step = width / Math.max(1, HISTORY_LENGTH - 1);
    const startX = width - step * (monitor.history.length - 1);
    ctx.beginPath();
    monitor.history.forEach((point, index) => {
      const x = startX + step * index;
      const y = height - (point[key] / max) * (height - 8);
      if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = stroke;
    ctx.lineWidth = 2;
    ctx.lineJoin = 'round';
    ctx.stroke();
    ctx.lineTo(width, height);
    ctx.lineTo(startX, height);
    ctx.closePath();
    const gradient = ctx.createLinearGradient(0, 0, 0, height);
    gradient.addColorStop(0, fill);
    gradient.addColorStop(1, 'transparent');
    ctx.fillStyle = gradient;
    ctx.fill();
  };
  plot('download', color('secondary'), `${color('secondary')}29`);
  plot('upload', color('primary'), `${color('primary')}21`);
}


function render() {
  $('traffic-limits-notice')?.classList.toggle('hidden', !monitor.snapshot?.details_limited);
  setStatus(monitor.snapshot);
  renderMetrics();
  renderConnections();
  renderLog();
  renderFocus();
  setTab(monitor.tab);
  drawChart();
  $('traffic-chart-empty')?.classList.toggle('hidden', monitor.running && monitor.history.length > 1);
}

function addHistoryPoint() {
  const metrics = selectedMetrics();
  monitor.history.push({ download: metrics.downloadBps || 0, upload: metrics.uploadBps || 0 });
  if (monitor.history.length > HISTORY_LENGTH) monitor.history.shift();
}

async function pollSnapshot() {
  if (!monitor.running || monitor.busy) return;
  try {
    const snapshot = await invoke('get_traffic_snapshot');
    monitor.snapshot = snapshot;
    monitor.running = Boolean(snapshot.running);
    addHistoryPoint();
  } catch (error) {
    monitor.snapshot = { ...(monitor.snapshot || {}), error: String(error) };
  }
  render();
}

async function toggleMonitor() {
  if (monitor.busy) return;
  monitor.busy = true;
  render();
  try {
    monitor.snapshot = await invoke(monitor.running ? 'stop_traffic_monitor' : 'start_traffic_monitor');
    monitor.running = Boolean(monitor.snapshot.running);
    if (monitor.running) {
      monitor.history = [];
      addHistoryPoint();
    }
  } catch (error) {
    monitor.running = false;
    monitor.snapshot = { running: false, error: String(error), connections: [], events: [] };
  } finally {
    monitor.busy = false;
    render();
  }
}

function setMode(mode) {
  monitor.mode = mode;
  document.querySelectorAll('.traffic-mode-btn').forEach((button) => {
    const active = button.dataset.mode === mode;
    button.setAttribute('aria-pressed', String(active));
    button.classList.toggle('bg-primary/15', active);
    button.classList.toggle('text-primary', active);
    button.classList.toggle('text-on-surface-variant/50', !active);
  });
  monitor.history = [];
  if (monitor.snapshot) addHistoryPoint();
  render();
}

export function openWarpTraffic() {
  monitor.focus = null;
  if ($('traffic-search')) $('traffic-search').value = '';
  monitor.hideSystem = false; // Internal relays belong to the same observed WARP route.
  if ($('traffic-hide-system')) $('traffic-hide-system').checked = false;
  setMode('warp');
  showSection('traffic');
}

export function refreshTrafficTranslations() {
  render();
}

export function initTrafficMonitor() {
  document.addEventListener('zapret:theme-changed', drawChart);
  $('traffic-toggle-btn')?.addEventListener('click', toggleMonitor);
  document.querySelectorAll('.traffic-mode-btn').forEach(button => button.addEventListener('click', () => setMode(button.dataset.mode)));
  $('traffic-search')?.addEventListener('input', () => {
    renderConnections();
    renderLog();
  });
  $('traffic-hide-system')?.addEventListener('change', (event) => {
    monitor.hideSystem = event.target.checked;
    render();
  });
  $('traffic-hide-local')?.addEventListener('change', event => { monitor.hideLocal = event.target.checked; render(); });
  $('traffic-focus-clear')?.addEventListener('click', clearFocus);
  $('traffic-tab-connections')?.addEventListener('click', () => setTab('connections'));
  $('traffic-tab-log')?.addEventListener('click', () => setTab('log'));
  const focusHandler = (event) => {
    const button = event.target.closest('[data-focus-type]');
    if (!button) return;
    setFocus(button.dataset.focusType, button.dataset.focusValue, button.dataset.focusLabel);
  };
  $('traffic-connections-list')?.addEventListener('click', focusHandler);
  $('traffic-log-list')?.addEventListener('click', focusHandler);
  window.addEventListener('resize', drawChart);

  monitor.timer = window.setInterval(() => {
    if (document.visibilityState === 'visible') pollSnapshot();
  }, 1000);
  setMode(monitor.mode);
}
