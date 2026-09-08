import { $, invoke, listen } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { showSection } from './navigation.js';
import { makeStatusRow } from './status-check.js';
import { showConfirm } from '../lib/dom.js';

let snapshot = null, busy = false;
export function moduleError(error) { return t(error?.code || error?.message || String(error)) + (error?.detail ? `: ${error.detail}` : ''); }
export async function installTelegram(onProgress = () => {}) {
  const unlisten = await listen('telegram-download-progress', ({ payload }) => onProgress(Number(payload)));
  try { await invoke('install_telegram'); } finally { unlisten(); }
}

function renderTelegramReport() {
  const dialog = $('telegram-status-dialog');
  if (!dialog || !dialog.open) return;
  const report = $('telegram-status-report');
  if (!report) return;
  report.replaceChildren();
  if (busy) {
    const loading = document.createElement('div');
    loading.className = 'rounded-xl bg-surface-container-high/70 p-5 text-sm text-on-surface-variant';
    loading.textContent = t('status_checking_realtime');
    report.append(loading);
    return;
  }
  const installed = snapshot?.installed === true;
  const running = snapshot?.running === true;

  const summary = document.createElement('div');
  summary.className = `rounded-xl p-4 ${installed ? (running ? 'bg-secondary/10 text-secondary' : 'bg-white/5 text-on-surface') : 'bg-white/5 text-on-surface-variant'}`;
  const title = document.createElement('div');
  title.className = 'font-headline text-base font-bold';
  title.textContent = installed ? (running ? t('tg_running', { port: snapshot.port }) : t('tg_stopped')) : t('tg_not_installed');
  summary.append(title);
  report.append(summary);

  const rows = [
    {
      icon: 'send',
      labelKey: 'nav_telegram',
      value: installed ? 'selected' : 'not_installed',
      valueLabel: installed ? t('warp_installed_short') : t('warp_not_installed'),
      detail: installed ? `v${snapshot.version}` : undefined,
    },
    {
      icon: 'power_settings_new',
      labelKey: 'status_label',
      value: installed ? (running ? 'running' : 'stopped') : 'not_installed',
      valueLabel: installed ? (running ? t('system_state_running') : t('system_state_stopped')) : t('system_state_not_installed'),
      detail: installed ? (running ? `127.0.0.1:${snapshot.port}` : t('tg_stopped')) : undefined,
    }
  ];
  if (installed) {
    rows.push({
      icon: 'settings_ethernet',
      labelKey: 'tg_port',
      value: 'selected',
      valueLabel: String(snapshot.port),
      detail: `127.0.0.1:${snapshot.port}`,
    });
  }
  report.append(...rows.map(makeStatusRow));
}

function render() {
  const installed = snapshot?.installed === true;
  $('nav-telegram').hidden = !installed;
  if (!installed && state.currentSectionId === 'telegram') showSection('settings');
  const disabled = busy || snapshot?.busy || !snapshot;
  $('telegram-install').hidden = installed;
  $('telegram-remove').hidden = !installed;
  if ($('telegram-settings-status-btn')) {
    $('telegram-settings-status-btn').hidden = !installed;
    $('telegram-settings-status-btn').disabled = disabled;
  }
  if ($('telegram-settings-open')) $('telegram-settings-open').hidden = true;
  for (const id of ['telegram-install', 'telegram-remove']) {
    if ($(id)) $(id).disabled = disabled;
  }
  $('telegram-install').textContent = t(busy ? 'tg_installing' : 'tg_install');
  $('telegram-settings-status').textContent = snapshot
    ? (installed
        ? t('tg_installed', { version: snapshot.version })
        : t('tg_download_size', { size: (snapshot.download_bytes / 1e6).toFixed(1) }))
    : t('tg_checking');
  $('telegram-status').textContent = snapshot?.running ? t('tg_running', { port: snapshot.port }) : t('tg_stopped');
  $('telegram-toggle').textContent = t(snapshot?.running ? 'tg_stop' : 'tg_start');
  $('telegram-toggle').disabled = disabled || !installed;
  for (const id of ['telegram-open', 'telegram-copy']) $(id).disabled = disabled || !installed || !snapshot?.running;
  for (const id of ['telegram-port', 'telegram-save']) $(id).disabled = disabled || !installed || snapshot?.running;
  if (snapshot && document.activeElement !== $('telegram-port')) $('telegram-port').value = snapshot.port;
  renderTelegramReport();
}
async function refresh() {
  try { snapshot = await invoke('get_telegram_status'); render(); }
  catch (error) { $('telegram-settings-message').textContent = moduleError(error); }
}
async function action(work, output = 'telegram-message') {
  if (busy) return;
  busy = true; $(output).textContent = ''; render();
  try { await work(); }
  catch (error) { $(output).textContent = moduleError(error); }
  finally { busy = false; await refresh(); }
}
export function initTelegram() {
  const statusDialog = $('telegram-status-dialog');
  if (statusDialog) document.body.append(statusDialog);
  $('telegram-settings-status-btn')?.addEventListener('click', async () => {
    if (busy) return;
    statusDialog?.showModal();
    renderTelegramReport();
    await refresh();
  });
  $('telegram-status-close')?.addEventListener('click', () => statusDialog?.close());
  statusDialog?.addEventListener('click', (event) => {
    if (event.target === statusDialog) statusDialog.close();
  });

  $('telegram-install').addEventListener('click', () => action(async () => {
    $('telegram-download-progress').hidden = false;
    try { await installTelegram(percent => { $('telegram-download-progress').value = percent; }); }
    finally { $('telegram-download-progress').hidden = true; }
  }, 'telegram-settings-message'));
  $('telegram-remove').addEventListener('click', async () => {
    const confirmed = await showConfirm(t('tg_remove_confirm'), t('tg_remove'));
    if (!confirmed) return;
    action(() => invoke('remove_telegram'), 'telegram-settings-message');
  });
  $('telegram-settings-open')?.addEventListener('click', () => showSection('telegram'));
  $('telegram-toggle').addEventListener('click', () => action(() => invoke(snapshot?.running ? 'stop_telegram' : 'start_telegram')));
  $('telegram-open').addEventListener('click', () => action(() => invoke('open_telegram')));
  $('telegram-copy').addEventListener('click', () => action(async () => { await navigator.clipboard.writeText(await invoke('telegram_link')); $('telegram-message').textContent = t('tg_copied'); }));
  $('telegram-port-form').addEventListener('submit', event => {
    event.preventDefault();
    const port = Number($('telegram-port').value);
    if (event.target.reportValidity()) action(() => invoke('set_telegram_port', { port }));
  });
  $('telegram-refresh-logs').addEventListener('click', () => action(async () => { $('telegram-logs').textContent = await invoke('telegram_logs') || t('tg_logs_empty'); }));
  onLangChange(() => { render(); renderTelegramReport(); });
  listen('telegram-module-changed', refresh).catch(console.error);
  refresh();
  setInterval(() => { if (!busy) refresh(); }, 3000);
}
