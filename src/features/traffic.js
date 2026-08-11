import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';

const monitor = {
  running: false,
  busy: false,
  mode: 'zapret',
  snapshot: null,
  timer: null,
  history: [],
  focus: null,
  tab: 'connections',
  hideSystem: true,
};

const HISTORY_LENGTH = 90;

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
  if (monitor.focus) {
    return (snapshot.connections || [])
      .filter(connectionMatchesFocus)
      .filter((connection) => monitor.mode !== 'zapret' || connection.zapret_candidate)
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
    if (monitor.mode === 'zapret' && !connection.zapret_candidate) return false;
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

  list.innerHTML = connections.map((connection) => {
    const downloadBps = monitor.mode === 'zapret' ? connection.zapret_download_bps : connection.download_bps;
    const uploadBps = monitor.mode === 'zapret' ? connection.zapret_upload_bps : connection.upload_bps;
    const downloadBytes = monitor.mode === 'zapret' ? connection.zapret_download_bytes : connection.download_bytes;
    const uploadBytes = monitor.mode === 'zapret' ? connection.zapret_upload_bytes : connection.upload_bytes;
    const endpoint = connection.remote_address.includes(':')
      ? `[${connection.remote_address}]:${connection.remote_port}`
      : `${connection.remote_address}:${connection.remote_port}`;
    const candidate = connection.zapret_candidate
      ? `<span class="px-1.5 py-0.5 rounded-md bg-primary/10 text-primary text-[8px] font-black uppercase">${escapeHtml(t('traffic_candidate'))}</span>`
      : '';
    return `<div class="grid grid-cols-[minmax(150px,1.4fr)_minmax(180px,1.5fr)_72px_90px_90px_90px] gap-3 items-center px-4 py-3 hover:bg-primary/[0.035] transition-colors text-[11px]">
      <div class="min-w-0">
        <div class="flex items-center gap-2 min-w-0">
          <button type="button" data-focus-type="process" data-focus-value="${escapeHtml(connection.process_name.toLowerCase())}" data-focus-label="${escapeHtml(connection.process_name)}" class="truncate font-semibold text-on-surface hover:text-tertiary transition-colors" title="${escapeHtml(t('traffic_track_process'))}">${escapeHtml(connection.process_name)}</button>${candidate}
        </div>
        <span class="text-[9px] text-on-surface-variant/40">PID ${connection.pid || '—'} · ${escapeHtml(connection.state)}</span>
      </div>
      <button type="button" data-focus-type="ip" data-focus-value="${escapeHtml(connection.remote_address)}" data-focus-label="${escapeHtml(connection.remote_address)}" class="min-w-0 text-left font-mono text-[10px] text-on-surface-variant/75 hover:text-tertiary truncate transition-colors" title="${escapeHtml(t('traffic_track_ip'))}: ${escapeHtml(endpoint)}">${escapeHtml(endpoint)}</button>
      <span class="text-[9px] font-black tracking-wider text-on-surface-variant/55">${escapeHtml(connection.protocol)}</span>
      <span class="text-right font-mono text-secondary">${formatBytes(downloadBps, '/s')}</span>
      <span class="text-right font-mono text-primary">${formatBytes(uploadBps, '/s')}</span>
      <span class="text-right font-mono text-on-surface-variant/55">${formatBytes(downloadBytes + uploadBytes)}</span>
    </div>`;
  }).join('');
}

function filteredEvents() {
  const query = ($('traffic-search')?.value || '').trim().toLowerCase();
  return (monitor.snapshot?.events || []).filter((event) => {
    if (monitor.mode === 'zapret' && !event.zapret_candidate) return false;
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
      <button type="button" data-focus-type="process" data-focus-value="${escapeHtml(event.process_name.toLowerCase())}" data-focus-label="${escapeHtml(event.process_name)}" class="text-left truncate text-on-surface hover:text-tertiary">${escapeHtml(event.process_name)} <span class="text-on-surface-variant/35">· ${event.pid || '—'}</span></button>
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

  ctx.strokeStyle = 'rgba(65, 71, 91, 0.18)';
  ctx.lineWidth = 1;
  for (let row = 1; row < 4; row += 1) {
    const y = (height / 4) * row;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(width, y);
    ctx.stroke();
  }

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
    gradient.addColorStop(1, 'rgba(7, 13, 31, 0)');
    ctx.fillStyle = gradient;
    ctx.fill();
  };
  plot('download', '#53ddfc', 'rgba(83, 221, 252, 0.16)');
  plot('upload', '#ba9eff', 'rgba(186, 158, 255, 0.13)');
}

function render() {
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
    button.classList.toggle('bg-primary/15', active);
    button.classList.toggle('text-primary', active);
    button.classList.toggle('text-on-surface-variant/50', !active);
  });
  monitor.history = [];
  if (monitor.snapshot) addHistoryPoint();
  render();
}

export function refreshTrafficTranslations() {
  render();
}

export function initTrafficMonitor() {
  $('traffic-toggle-btn')?.addEventListener('click', toggleMonitor);
  $('traffic-mode-zapret')?.addEventListener('click', () => setMode('zapret'));
  $('traffic-mode-all')?.addEventListener('click', () => setMode('all'));
  $('traffic-search')?.addEventListener('input', () => {
    renderConnections();
    renderLog();
  });
  $('traffic-hide-system')?.addEventListener('change', (event) => {
    monitor.hideSystem = event.target.checked;
    render();
  });
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
  render();
}
