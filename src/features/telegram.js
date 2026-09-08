import { $, invoke, listen } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { showSection } from './navigation.js';

let snapshot = null, busy = false;
export function moduleError(error) { return t(error?.code || error?.message || String(error)) + (error?.detail ? `: ${error.detail}` : ''); }
export async function installTelegram(onProgress = () => {}) {
  const unlisten = await listen('telegram-download-progress', ({ payload }) => onProgress(Number(payload)));
  try { await invoke('install_telegram'); } finally { unlisten(); }
}
function render() {
  const installed = snapshot?.installed === true;
  $('nav-telegram').hidden = !installed;
  if (!installed && state.currentSectionId === 'telegram') showSection('settings');
  const disabled = busy || snapshot?.busy || !snapshot;
  $('telegram-install').hidden = installed;
  $('telegram-remove').hidden = !installed;
  $('telegram-settings-open').hidden = !installed;
  for (const id of ['telegram-install', 'telegram-remove', 'telegram-settings-open']) $(id).disabled = disabled;
  $('telegram-install').textContent = t(busy ? 'tg_installing' : 'tg_install');
  $('telegram-settings-status').textContent = snapshot ? installed ? t('tg_installed', { version: snapshot.version }) : t('tg_download_size', { size: (snapshot.download_bytes / 1e6).toFixed(1) }) : t('tg_checking');
  $('telegram-status').textContent = snapshot?.running ? t('tg_running', { port: snapshot.port }) : t('tg_stopped');
  $('telegram-toggle').textContent = t(snapshot?.running ? 'tg_stop' : 'tg_start');
  $('telegram-toggle').disabled = disabled || !installed;
  for (const id of ['telegram-open', 'telegram-copy']) $(id).disabled = disabled || !installed || !snapshot?.running;
  for (const id of ['telegram-port', 'telegram-save']) $(id).disabled = disabled || !installed || snapshot?.running;
  if (snapshot && document.activeElement !== $('telegram-port')) $('telegram-port').value = snapshot.port;
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
  $('telegram-install').addEventListener('click', () => action(async () => {
    $('telegram-download-progress').hidden = false;
    try { await installTelegram(percent => { $('telegram-download-progress').value = percent; }); }
    finally { $('telegram-download-progress').hidden = true; }
  }, 'telegram-settings-message'));
  $('telegram-remove').addEventListener('click', () => action(() => invoke('remove_telegram'), 'telegram-settings-message'));
  $('telegram-settings-open').addEventListener('click', () => showSection('telegram'));
  $('telegram-toggle').addEventListener('click', () => action(() => invoke(snapshot?.running ? 'stop_telegram' : 'start_telegram')));
  $('telegram-open').addEventListener('click', () => action(() => invoke('open_telegram')));
  $('telegram-copy').addEventListener('click', () => action(async () => { await navigator.clipboard.writeText(await invoke('telegram_link')); $('telegram-message').textContent = t('tg_copied'); }));
  $('telegram-port-form').addEventListener('submit', event => {
    event.preventDefault();
    const port = Number($('telegram-port').value);
    if (event.target.reportValidity()) action(() => invoke('set_telegram_port', { port }));
  });
  $('telegram-refresh-logs').addEventListener('click', () => action(async () => { $('telegram-logs').textContent = await invoke('telegram_logs') || t('tg_logs_empty'); }));
  onLangChange(render);
  listen('telegram-module-changed', refresh).catch(console.error);
  refresh();
  setInterval(() => { if (!busy) refresh(); }, 3000);
}
