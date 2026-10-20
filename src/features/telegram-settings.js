import { $, invoke } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { showConfirm } from '../lib/dom.js';

let settings = null, dirty = false, controlsDisabled = true;
const fields = ['telegram-start-with-zapret', 'telegram-host', 'telegram-port', 'telegram-dc-ips', 'telegram-cfproxy', 'telegram-cf-domains', 'telegram-worker', 'telegram-worker-domains'];
function updateSaveState() {
  $('telegram-save').disabled = controlsDisabled || !dirty;
  $('telegram-unsaved').hidden = !dirty;
}
function syncRelayFields() {
  $('telegram-worker-domains').required = $('telegram-worker').checked;
  for (const id of ['telegram-start-with-zapret', 'telegram-cfproxy', 'telegram-worker']) {
    const input = $(id);
    const track = input.nextElementSibling;
    track.classList.toggle('is-on', input.checked);
    track.classList.toggle('is-off', !input.checked);
  }
}
function initInfoButtons() {
  const dialog = $('telegram-info-dialog');
  document.body.append(dialog);
  let activeButton = null;
  const refreshInfo = () => {
    document.querySelectorAll('[data-telegram-info]').forEach(button => {
      const label = `${t('warp_info')}: ${t(button.dataset.telegramInfoTitle)}`;
      button.setAttribute('aria-label', label);
      button.title = label;
    });
    if (activeButton) {
      $('telegram-info-title').textContent = t(activeButton.dataset.telegramInfoTitle);
      $('telegram-info-text').textContent = activeButton.dataset.telegramInfo.split(',').map(key => t(key)).join('\n\n');
    }
  };
  document.querySelectorAll('[data-telegram-info]').forEach(button => {
    button.addEventListener('click', () => {
      activeButton = button;
      refreshInfo();
      dialog.showModal();
    });
  });
  $('telegram-info-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => { if (event.target === dialog) dialog.close(); });
  onLangChange(refreshInfo);
  refreshInfo();
}
function show(value) {
  settings = value; dirty = false;
  $('telegram-host').value = value.host;
  $('telegram-port').value = value.port;
  $('telegram-secret').value = value.secret;
  $('telegram-start-with-zapret').checked = Boolean(value.start_with_zapret);
  $('telegram-dc-ips').value = Object.entries(value.dc_ips).map(([dc, ip]) => `${dc}:${ip}`).join('\n');
  $('telegram-cfproxy').checked = value.cfproxy;
  $('telegram-cf-domains').value = value.cfproxy_domains.join('\n');
  $('telegram-worker').checked = value.worker;
  $('telegram-worker-domains').value = value.worker_domains.join('\n');
  syncRelayFields();
  updateSaveState();
}
export async function refreshTelegramSettings(installed) {
  if (!installed) { settings = null; dirty = false; updateSaveState(); return; }
  if (!settings) show(await invoke('get_telegram_config'));
}
export function setTelegramSettingsBusy(disabled, running = false) {
  controlsDisabled = Boolean(disabled);
  // Help remains available while the proxy runs; only editable controls lock.
  $('telegram-config-fields').disabled = false;
  for (const id of [...fields, 'telegram-secret', 'telegram-regenerate']) $(id).disabled = controlsDisabled;
  updateSaveState();
  $('telegram-settings-lock').hidden = !running;
}
export function invalidateTelegramSettings() { if (!dirty) settings = null; }
export function parseDcIps(text) {
  const result = {};
  for (const line of text.split(/\r?\n/).map(line => line.trim()).filter(Boolean)) {
    const match = line.match(/^(\d+)\s*(?::|->|→)\s*(\d{1,3}(?:\.\d{1,3}){3})$/);
    if (!match || +match[1] < 1 || +match[1] > 32767 || match[2].split('.').some(n => +n > 255) || Object.hasOwn(result, +match[1])) throw new Error(t('tg_invalid_dc'));
    result[+match[1]] = match[2];
  }
  return result;
}
export function initTelegramSettings(action) {
  initInfoButtons();
  for (const id of fields) $(id).addEventListener('input', () => {
    dirty = true;
    syncRelayFields();
    updateSaveState();
    if ($('telegram-message').dataset.state === 'success') {
      $('telegram-message').textContent = '';
      delete $('telegram-message').dataset.state;
    }
  });
  $('telegram-port-form').addEventListener('invalid', event => {
    if (event.target.closest('#telegram-advanced-settings')) $('telegram-advanced-settings').open = true;
  }, true);
  $('telegram-port-form').addEventListener('submit', event => {
    event.preventDefault();
    if (controlsDisabled || !event.target.reportValidity() || !settings) return;
    let value;
    try {
      value = { ...settings, start_with_zapret: $('telegram-start-with-zapret').checked, host: $('telegram-host').value.trim(), port: Number($('telegram-port').value), dc_ips: parseDcIps($('telegram-dc-ips').value),
        cfproxy: $('telegram-cfproxy').checked, cfproxy_domains: $('telegram-cf-domains').value.split(/\s+/).filter(Boolean),
        worker: $('telegram-worker').checked, worker_domains: $('telegram-worker-domains').value.split(/\s+/).filter(Boolean) };
    } catch (error) { $('telegram-message').dataset.state = 'error'; $('telegram-message').textContent = error.message; return; }
    action(async () => { show(await invoke('save_telegram_config', { settings: value })); $('telegram-message').dataset.state = 'success'; $('telegram-message').textContent = t('tg_config_saved'); });
  });
  $('telegram-regenerate').addEventListener('click', async () => {
    if (!await showConfirm(t('tg_regenerate_confirm'), t('tg_regenerate'))) return;
    action(async () => {
      const value = await invoke('regenerate_telegram_secret');
      // Keep unsaved address/DC/relay edits while replacing only the secret.
      if (settings) settings.secret = value.secret;
      $('telegram-secret').value = value.secret;
      $('telegram-message').dataset.state = 'success';
      $('telegram-message').textContent = t('tg_secret_changed');
    });
  });
}
