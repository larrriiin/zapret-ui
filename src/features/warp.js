import { $, invoke } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import markup from '../components/warp.html?raw';
import { initWarpModes, renderWarpModes, closeWarpModes } from './warp-mode.js';
import { setWarpSummary } from './connection-summary.js';

export const WARP_MODE_KEYS = {
  doh: 'warp_mode_doh', dot: 'warp_mode_dot', warp: 'warp_mode_warp',
  'warp+dot': 'warp_mode_warp_dot', 'warp+doh': 'warp_mode_warp_doh',
  proxy: 'warp_mode_proxy', tunnel_only: 'warp_mode_tunnel_only',
};
let snapshot = null;
let busy = false;
let polling = null;
let installing = false;
let operation = null;
let operationError = null;
let requestError = null;
let checking = false;
let hero, zapret, heading, zapretHeader, statusHeading;

function errorText(error) {
  const code = error?.code || 'warp_process';
  const translated = t(code);
  return `${translated === code ? t('warp_process') : translated}${error?.detail ? ` — ${error.detail}` : ''}`;
}

function layout(installed) {
  if (hero.classList.contains('has-warp') === installed) return;
  hero.classList.toggle('has-warp', installed);
  heading.hidden = !installed;
  $('warp-card').hidden = !installed;
  zapretHeader.hidden = !installed;
  if (installed) zapretHeader.append(statusHeading);
  else zapret.prepend(statusHeading);
}

function render() {
  const s = snapshot;
  setWarpSummary(Boolean(s?.installed && s?.connected && !requestError && !s?.error));
  if (s) layout(s.installed);
  const status = operation || (requestError || operationError ? 'error' : s?.state) || 'disconnected';
  const locked = busy || status === 'connecting' || status === 'disconnecting';
  $('warp-state').textContent = t(`warp_state_${status}`);
  $('warp-card').dataset.state = status;
  $('warp-connect-label').textContent = operation ? t(`warp_state_${operation}`) : t(s?.connected ? 'warp_disconnect' : 'warp_connect');
  $('warp-connect').disabled = locked || !s?.installed || !s?.modes?.length;
  $('warp-refresh').disabled = busy || checking;
  const mode = $('warp-mode');
  const values = s?.modes || [];
  const selected = s?.mode;
  const options = values.map(value => [value, t(WARP_MODE_KEYS[value] || 'warp_unknown')]);
  if (!selected || !values.includes(selected)) options.unshift([selected || '', t('warp_unknown')]);
  const signature = JSON.stringify(options);
  if (mode.dataset.options !== signature) {
    mode.replaceChildren(...options.map(([value, label]) => new Option(label, value)));
    mode.dataset.options = signature;
  }
  mode.value = selected || '';
  mode.disabled = locked || !values.length;
  renderWarpModes(options, selected, mode.disabled);
  const proxy = s?.proxy;
  $('warp-proxy').hidden = !proxy;
  if (proxy) {
    $('warp-proxy-state').textContent = t(proxy.active && !requestError ? 'warp_state_connected' : 'warp_state_disconnected');
    $('warp-proxy-endpoint').textContent = `${proxy.kind || t('warp_unknown')} · ${proxy.address}:${proxy.port ?? t('warp_unknown')}`;
    if (document.activeElement !== $('warp-port') && !busy) $('warp-port').value = proxy.port ?? '';
    $('warp-port-form').hidden = !proxy.port_editable;
  }
  $('warp-port').disabled = locked;
  $('warp-port-save').disabled = locked;
  const error = operationError || requestError || s?.error;
  $('warp-error').hidden = !error;
  $('warp-error').textContent = error ? errorText(error) : '';
  $('warp-install').hidden = Boolean(s?.installed);
  $('warp-install').disabled = installing || !s || Boolean(requestError);
  $('warp-install').textContent = t(installing ? 'warp_installing' : 'warp_install');
  $('warp-settings-refresh').disabled = busy || checking;
  $('warp-settings-status').textContent = requestError || s?.error ? errorText(requestError || s.error) : !s ? t('warp_detecting') : s.installed ? `${t(`warp_state_${s.state}`)} · ${s.version || 'Cloudflare WARP'}` : t('warp_not_installed');
  $('warp-settings-description').textContent = t(s?.installed ? 'warp_settings_connected_desc' : 'warp_settings_install_desc');
  $('warp-install-details').hidden = Boolean(s?.installed);
  renderReport();
}

function renderReport() {
  if (!$('warp-status-dialog').open) return;
  const report = $('warp-status-report');
  report.replaceChildren();
  report.setAttribute('aria-busy', String(checking));
  if (checking) { report.textContent = t('status_checking_realtime'); return; }
  const error = requestError || snapshot?.error;
  if (error) {
    const message = document.createElement('p');
    message.className = 'warp-error'; message.textContent = errorText(error); report.append(message);
  }
  const rows = [[t('warp_client'), snapshot?.installed ? t('warp_installed_short') : t('warp_not_installed')]];
  if (snapshot?.installed) {
    rows.push([t('status_label'), t(`warp_state_${error ? 'error' : snapshot.state}`)],
      [t('warp_mode'), t(WARP_MODE_KEYS[snapshot.mode] || 'warp_unknown')],
      [t('warp_client_version'), snapshot.version || t('warp_unknown')]);
    if (snapshot.proxy) rows.push([t('warp_mode_proxy'), `${snapshot.proxy.kind || t('warp_unknown')} · ${snapshot.proxy.address}:${snapshot.proxy.port ?? t('warp_unknown')}`]);
  }
  if (snapshot) for (const [label, value] of rows) {
    const row = document.createElement('div'); row.className = 'warp-report-row';
    const name = document.createElement('span'); name.textContent = label;
    const text = document.createElement('strong'); text.textContent = value;
    row.append(name, text); report.append(row);
  }
}

async function checkStatus() {
  if (busy || checking) return;
  closeWarpModes();
  checking = true; operationError = null;
  $('warp-status-dialog').showModal(); render();
  try { await refresh(); }
  finally { checking = false; render(); }
}

async function refresh() {
  if (busy || polling) return polling;
  polling = (async () => {
    try {
      snapshot = await invoke('get_warp_status');
      requestError = null;
      operationError = null;
    } catch (error) {
      if (checking || error?.code !== 'warp_busy') requestError = error;
    } finally { render(); }
  })();
  try { await polling; } finally { polling = null; }
}

async function act(command, args, state = null) {
  if (busy) return;
  busy = true; operation = state; operationError = null; render();
  try { await polling; snapshot = await invoke(command, args); requestError = null; }
  catch (error) { operationError = error; }
  finally { busy = false; operation = null; render(); }
}

export function initWarp() {
  hero = document.querySelector('#section-home > section');
  zapret = hero.querySelector(':scope > .relative');
  zapret.classList.add('zapret-connection');
  statusHeading = $('hero-status').parentElement;
  zapretHeader = document.createElement('header');
  zapretHeader.className = 'provider-heading';
  zapretHeader.hidden = true;
  zapretHeader.innerHTML = '<span class="material-symbols-outlined zapret-provider-icon" aria-hidden="true">tune</span><div><h3>ZAPRET</h3><p data-i18n="warp_zapret_subtitle"></p></div>';
  zapret.prepend(zapretHeader);
  const fragment = document.createElement('template');
  fragment.innerHTML = markup;
  hero.append(fragment.content);
  document.body.append($('warp-status-dialog'));
  heading = hero.querySelector('.connection-heading');
  hero.prepend(heading);
  // Translate only the inserted content: translating the whole page here would
  // reset Zapret's live status/button labels to their initial HTML values.
  for (const root of [heading, zapretHeader, $('warp-card'), $('warp-status-dialog')]) {
    root.querySelectorAll('[data-i18n]').forEach(el => {
      if (el.dataset.i18nAttr) el.setAttribute(el.dataset.i18nAttr, t(el.dataset.i18n));
      else el.textContent = t(el.dataset.i18n);
    });
  }
  initWarpModes();
  $('warp-status-close').addEventListener('click', () => $('warp-status-dialog').close());
  onLangChange(render);
  $('warp-connect').addEventListener('click', () => act(snapshot?.connected ? 'disconnect_warp' : 'connect_warp', undefined, snapshot?.connected ? 'disconnecting' : 'connecting'));
  $('warp-mode').addEventListener('change', event => act('set_warp_mode', { mode: event.target.value }));
  $('warp-port-form').addEventListener('submit', event => {
    event.preventDefault();
    if ($('warp-port-form').reportValidity()) act('set_warp_proxy_port', { port: Number($('warp-port').value) });
  });
  for (const id of ['warp-refresh', 'warp-settings-refresh']) $(id).addEventListener('click', checkStatus);
  $('warp-install').addEventListener('click', async () => {
    if (installing) return;
    installing = true; render();
    $('warp-install-result').textContent = '';
    try { await invoke('install_warp'); $('warp-install-result').textContent = t('warp_install_done'); }
    catch (error) { $('warp-install-result').textContent = errorText(error); }
    finally { installing = false; await refresh(); render(); }
  });
  // One loop per webview; the backend gate also serializes across windows.
  async function poll() { await refresh(); window.setTimeout(poll, 3000); }
  poll();
}
