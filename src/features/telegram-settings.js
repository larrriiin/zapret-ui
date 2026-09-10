import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { showConfirm } from '../lib/dom.js';

let settings = null, dirty = false;
const fields = ['telegram-start-with-zapret', 'telegram-host', 'telegram-port', 'telegram-dc-ips', 'telegram-cfproxy', 'telegram-cf-domains', 'telegram-worker', 'telegram-worker-domains'];
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
}
export async function refreshTelegramSettings(installed) {
  if (!installed) { settings = null; dirty = false; return; }
  if (!settings) show(await invoke('get_telegram_config'));
}
export function setTelegramSettingsBusy(disabled) {
  $('telegram-config-fields').disabled = disabled;
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
  for (const id of fields) $(id).addEventListener('input', () => { dirty = true; });
  $('telegram-port-form').addEventListener('submit', event => {
    event.preventDefault();
    if (!event.target.reportValidity() || !settings) return;
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
