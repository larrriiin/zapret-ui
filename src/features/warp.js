import { $, invoke } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import markup from '../components/warp.html?raw';
import { initWarpModes, renderWarpModes, closeWarpModes } from './warp-mode.js';
import { setWarpSummary } from './connection-summary.js';
import { makeStatusRow } from './status-check.js';
import { showSection } from './navigation.js';

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
let cancelRequested = false;
let sitesDirty = false;
let sitesLoaded = false;
let hero, zapret, heading, zapretHeader, statusHeading;

function errorText(error) {
  const code = error?.code || 'warp_process';
  const translated = t(code);
  return `${translated === code ? t('warp_process') : translated}${error?.detail ? ` — ${error.detail}` : ''}`;
}

function siteDomains() {
  return $('warp-sites-domains').value.split(/\r?\n/).map(domain => domain.trim()).filter(Boolean);
}

function normalizeSiteDomain(value) {
  const candidate = value.trim();
  if (!candidate) return '';
  try {
    const source = /^[a-z][a-z\d+.-]*:\/\//i.test(candidate)
      ? candidate
      : `https://${candidate.replace(/^\/+/, '')}`;
    return new URL(source).hostname.replace(/\.$/, '').toLowerCase();
  } catch {
    return candidate;
  }
}

function normalizeSiteDomains(value) {
  return [...new Set(value.split(/\r?\n/).map(normalizeSiteDomain).filter(Boolean))];
}

function openWarpInfo(kind, path = '') {
  const dialog = $('warp-settings-info-dialog');
  const title = $('warp-settings-info-title');
  const text = $('warp-settings-info-text');
  const pathNode = $('warp-settings-info-path');
  const icon = $('warp-settings-info-icon');
  const fileName = path.split(/[\\/]/).pop();
  if (kind === 'sites') {
    title.textContent = t('warp_sites_title'); text.textContent = t('warp_sites_hint'); icon.textContent = 'language'; pathNode.hidden = true;
  } else if (kind === 'apps') {
    title.textContent = t('warp_apps_title'); text.textContent = t('warp_apps_scope'); icon.textContent = 'apps'; pathNode.hidden = true;
  } else {
    title.textContent = fileName; text.textContent = t('warp_apps_path_caption'); icon.textContent = 'description'; pathNode.textContent = path; pathNode.hidden = false;
  }
  if (!dialog.open) dialog.showModal();
}

function layout(installed) {
  $('nav-warp-settings').hidden = !installed;
  if (hero.classList.contains('has-warp') === installed) return;
  hero.classList.toggle('has-warp', installed);
  heading.hidden = !installed;
  $('warp-card').hidden = !installed;
  zapretHeader.hidden = !installed;
  (installed ? zapretHeader.querySelector('h3') : $('strategy-title')).append($('zapret-info-btn'));
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
  const connecting = operation === 'connecting' || (!busy && s?.state === 'connecting');
  $('warp-connect-label').textContent = connecting ? t('warp_cancel_connect') : operation ? t(`warp_state_${operation}`) : t(s?.connected ? 'warp_disconnect' : 'warp_connect');
  $('warp-connect').disabled = (!connecting && locked) || !s?.installed || (!s?.modes?.length && !s?.connected && !connecting);
  $('warp-connect').dataset.action = s?.connected ? 'stop' : 'start';
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
  $('warp-open-settings').hidden = selected !== 'proxy';
  const proxy = s?.proxy;
  $('warp-proxy').hidden = !proxy;
  if (proxy) {
    $('warp-proxy-endpoint').textContent = `${proxy.kind || t('warp_unknown')} · ${proxy.address}:${proxy.port ?? t('warp_unknown')}`;
    if (document.activeElement !== $('warp-port') && !busy) $('warp-port').value = proxy.port ?? '';
    $('warp-port-form').hidden = !proxy.port_editable;
  }
  const apps = s?.applications || { paths: [], enabled: false };
  $('warp-port').disabled = locked || Boolean(s?.sites?.enabled) || apps.enabled;
  $('warp-port-save').disabled = $('warp-port').disabled;
  const sites = s?.sites;
  const siteInput = $('warp-sites-domains');
  if (sites && (!sitesLoaded || !sitesDirty) && document.activeElement !== siteInput) {
    siteInput.value = sites.domains.join('\n');
    sitesLoaded = true;
  }
  const sitesEnabled = Boolean(sites?.enabled);
  const canEnableSites = Boolean(proxy?.active && proxy.kind === 'SOCKS5' && proxy.port && !s?.error && !requestError);
  siteInput.disabled = busy;
  $('warp-sites-add').disabled = busy || !s?.installed;
  $('warp-sites-toggle').disabled = locked || (!sitesEnabled && (!canEnableSites || !siteInput.value.trim()));
  $('warp-sites-toggle').textContent = t(sitesEnabled ? 'warp_sites_disable' : 'warp_sites_enable');
  $('warp-sites-toggle').dataset.enabled = String(sitesEnabled);
  $('warp-sites-toggle').setAttribute('aria-pressed', String(sitesEnabled));
  const domains = siteDomains();
  $('warp-sites-empty').hidden = domains.length > 0;
  const siteList = $('warp-sites-list');
  const siteSignature = JSON.stringify([domains, locked, t('warp_sites_remove')]);
  if (siteList.dataset.signature !== siteSignature) {
    siteList.dataset.signature = siteSignature;
    siteList.replaceChildren(...domains.map(domain => {
      const row = document.createElement('li'); row.className = 'warp-rule-card';
      const icon = document.createElement('span'); icon.className = 'material-symbols-outlined'; icon.setAttribute('aria-hidden', 'true'); icon.textContent = 'language';
      const name = document.createElement('span'); name.className = 'warp-rule-name'; name.textContent = domain;
      const actions = document.createElement('div'); actions.className = 'warp-card-actions';
      const remove = document.createElement('button'); remove.type = 'button'; remove.className = 'warp-icon-button'; remove.title = t('warp_sites_remove'); remove.setAttribute('aria-label', `${t('warp_sites_remove')}: ${domain}`);
      remove.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">close</span>'; remove.disabled = locked;
      remove.addEventListener('click', () => act('set_warp_sites', { domains: siteDomains().filter(item => item !== domain), enabled: Boolean(snapshot?.sites?.enabled) }));
      actions.append(remove); row.append(icon, name, actions); return row;
    }));
  }
  $('warp-page-status').textContent = t(`warp_state_${s?.state || 'disconnected'}`);
  $('warp-page-status').dataset.state = s?.state || 'disconnected';
  $('warp-page-connection').textContent = proxy ? t('warp_mode_proxy') : t('warp_sites_requires_proxy');
  $('warp-apps-empty').hidden = apps.paths.length > 0;
  $('warp-apps-add').disabled = locked || apps.enabled || !s?.installed;
  $('warp-apps-toggle').disabled = locked || (!apps.enabled && (!canEnableSites || !apps.paths.length));
  $('warp-apps-toggle').textContent = t(apps.enabled ? 'warp_apps_disable' : 'warp_apps_enable');
  $('warp-apps-toggle').dataset.enabled = String(apps.enabled);
  $('warp-apps-toggle').setAttribute('aria-pressed', String(apps.enabled));
  $('warp-apps-counters').textContent = t('warp_apps_counters', { active: apps.active_connections || 0, total: apps.total_connections || 0, failed: apps.failed_connections || 0 });
  $('warp-apps-error').hidden = !apps.error;
  $('warp-apps-error').textContent = apps.error ? `${t('warp_apps_error')} — ${apps.error}` : '';
  const list = $('warp-apps-list');
  const listSignature = JSON.stringify([apps.paths, locked, apps.enabled, t('warp_apps_remove'), t('warp_apps_path')]);
  if (list.dataset.signature !== listSignature) {
    list.dataset.signature = listSignature;
    list.replaceChildren(...apps.paths.map(path => {
      const row = document.createElement('li'); row.className = 'warp-rule-card';
      const icon = document.createElement('span'); icon.className = 'material-symbols-outlined'; icon.setAttribute('aria-hidden', 'true'); icon.textContent = 'description';
      const name = document.createElement('span'); name.className = 'warp-rule-name'; name.textContent = path.split(/[\\/]/).pop();
      const actions = document.createElement('div'); actions.className = 'warp-card-actions';
      const details = document.createElement('button'); details.type = 'button'; details.className = 'warp-icon-button'; details.title = t('warp_apps_path'); details.setAttribute('aria-label', `${t('warp_apps_path')}: ${name.textContent}`);
      details.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">info</span>'; details.addEventListener('click', () => openWarpInfo('path', path));
      const remove = document.createElement('button'); remove.type = 'button'; remove.className = 'warp-icon-button'; remove.title = t('warp_apps_remove'); remove.setAttribute('aria-label', `${t('warp_apps_remove')}: ${name.textContent}`);
      remove.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">close</span>'; remove.disabled = locked || apps.enabled;
      remove.addEventListener('click', () => act('set_warp_applications', { paths: apps.paths.filter(item => item !== path), enabled: false }));
      actions.append(details, remove); row.append(icon, name, actions); return row;
    }));
  }
  const error = operationError || requestError || s?.error;
  $('warp-error').hidden = !error;
  $('warp-error').textContent = error ? errorText(error) : '';
  $('warp-page-error').hidden = !error;
  $('warp-page-error').textContent = error ? errorText(error) : '';
  $('warp-install').hidden = Boolean(s?.installed);
  $('warp-install').disabled = installing || !s || Boolean(requestError);
  $('warp-install').textContent = t(installing ? 'warp_installing' : 'warp_install');
  $('warp-settings-refresh').disabled = busy || checking;
  if ($('warp-settings-remove')) {
    $('warp-settings-remove').hidden = !s?.installed;
    $('warp-settings-remove').disabled = busy || checking;
  }
  const warpVer = s?.version ? s.version.replace(/^warp-cli\s*/i, '') : '';
  $('warp-settings-status').textContent = requestError || s?.error
    ? errorText(requestError || s.error)
    : !s
      ? t('warp_detecting')
      : s.installed
        ? (warpVer ? t('warp_installed_cli', { version: warpVer }) : t('warp_installed_cli_no_ver'))
        : t('warp_not_installed');
  $('warp-settings-description').textContent = t('warp_subtitle');
  if ($('warp-install-details')) $('warp-install-details').hidden = true;
  renderReport();
}

function renderReport() {
  if (!$('warp-status-dialog').open) return;
  const report = $('warp-status-report');
  report.replaceChildren();
  report.setAttribute('aria-busy', String(checking));
  if (checking) {
    const loading = document.createElement('div');
    loading.className = 'rounded-xl bg-surface-container-high/70 p-5 text-sm text-on-surface-variant';
    loading.textContent = t('status_checking_realtime'); report.append(loading); return;
  }
  const error = requestError || snapshot?.error;
  if (snapshot?.installed) {
    const summary = document.createElement('div');
    summary.className = `rounded-xl p-4 ${error ? 'bg-tertiary/10 text-tertiary' : snapshot.connected ? 'bg-secondary/10 text-secondary' : 'bg-white/5 text-on-surface'}`;
    const title = document.createElement('div');
    title.className = 'font-headline text-base font-bold';
    title.textContent = t(`warp_state_${error ? 'error' : snapshot.state}`);
    summary.append(title); report.append(summary);
  }
  if (error) {
    const message = document.createElement('p');
    message.className = 'warp-error'; message.textContent = errorText(error); report.append(message);
  }
  const rows = [{ icon: 'cloud', labelKey: 'warp_client', value: snapshot?.installed ? 'selected' : 'not_installed', valueLabel: snapshot?.installed ? t('warp_installed_short') : t('warp_not_installed'), detail: snapshot?.installed ? snapshot.version || t('warp_unknown') : undefined }];
  if (snapshot?.installed) {
    rows.push({ icon: 'power_settings_new', labelKey: 'status_label', value: error ? 'unknown' : snapshot.connected ? 'running' : 'stopped', detail: t(`warp_state_${error ? 'error' : snapshot.state}`) },
      { icon: 'route', labelKey: 'warp_mode', value: snapshot.mode ? 'selected' : 'unknown', detail: t(WARP_MODE_KEYS[snapshot.mode] || 'warp_unknown') });
    if (snapshot.proxy) rows.push({ icon: 'settings_ethernet', labelKey: 'warp_mode_proxy', value: snapshot.proxy.active ? 'running' : 'stopped', detail: `${snapshot.proxy.kind || t('warp_unknown')} · ${snapshot.proxy.address}:${snapshot.proxy.port ?? t('warp_unknown')}` });
  }
  if (snapshot) report.append(...rows.map(makeStatusRow));
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
    } catch (error) {
      if (checking || error?.code !== 'warp_busy') requestError = error;
    } finally { render(); }
  })();
  try { await polling; } finally { polling = null; }
}

async function act(command, args, state = null) {
  if (busy) return;
  busy = true; operation = state; operationError = null; render();
  try {
    await polling; snapshot = await invoke(command, args); requestError = null;
    if (command === 'set_warp_sites') {
      sitesDirty = false;
      $('warp-sites-domains').value = (snapshot.sites?.domains || []).join('\n');
    }
  }
  catch (error) { operationError = error; }
  finally {
    if (cancelRequested) {
      // The CLI gate serializes commands. Finish the pending request, then
      // disconnect even if that request failed or has not established a tunnel.
      cancelRequested = false;
      operation = 'disconnecting'; render();
      try {
        snapshot = await invoke('disconnect_warp');
        requestError = null; operationError = null;
      } catch (error) { operationError = error; }
    }
    busy = false; operation = null; render();
  }
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
  const warpInfoDialog = $('warp-settings-info-dialog');
  document.body.append(warpInfoDialog);
  $('warp-settings-info-close').addEventListener('click', () => warpInfoDialog.close());
  warpInfoDialog.addEventListener('click', event => { if (event.target === warpInfoDialog) warpInfoDialog.close(); });
  $('warp-sites-info').addEventListener('click', () => openWarpInfo('sites'));
  $('warp-apps-info').addEventListener('click', () => openWarpInfo('apps'));
  $('warp-open-settings').addEventListener('click', () => showSection('warp-settings'));
  const sitesDialog = $('warp-sites-dialog');
  document.body.append(sitesDialog);
  $('warp-sites-dialog-close').addEventListener('click', () => sitesDialog.close());
  sitesDialog.addEventListener('click', event => { if (event.target === sitesDialog) sitesDialog.close(); });
  $('warp-copy-proxy').addEventListener('click', async () => {
    const address = $('warp-proxy-endpoint').textContent;
    try { await navigator.clipboard.writeText(address); }
    catch { /* Clipboard access is unavailable in some embedded webviews. */ }
  });
  $('warp-apps-toggle').addEventListener('click', () => act('set_warp_applications', { paths: snapshot?.applications?.paths || [], enabled: !snapshot?.applications?.enabled }));
  $('warp-apps-add').addEventListener('click', async () => {
    if (busy) return;
    busy = true; operationError = null; render();
    try {
      await polling;
      const paths = await invoke('choose_warp_applications');
      if (paths.length) snapshot = await invoke('set_warp_applications', { paths: [...(snapshot?.applications?.paths || []), ...paths], enabled: false });
    } catch (error) { operationError = error; }
    finally { busy = false; render(); }
  });
  const saveSiteDomains = domains => act('set_warp_sites', {
    domains,
    enabled: Boolean(snapshot?.sites?.enabled),
  });
  $('warp-sites-add').addEventListener('click', () => {
    $('warp-sites-input').value = '';
    sitesDialog.showModal();
    $('warp-sites-input').focus();
  });
  const addSites = () => {
    const added = normalizeSiteDomains($('warp-sites-input').value);
    const existing = siteDomains();
    const domains = [...new Set([...existing, ...added])];
    if (domains.length !== existing.length) saveSiteDomains(domains);
    sitesDialog.close();
  };
  $('warp-sites-dialog-add').addEventListener('click', addSites);
  $('warp-sites-toggle').addEventListener('click', () => {
    const enabled = !snapshot?.sites?.enabled;
    act('set_warp_sites', { domains: enabled ? siteDomains() : snapshot.sites.domains, enabled });
  });
  $('warp-status-close').addEventListener('click', () => $('warp-status-dialog').close());
  const removeDialog = $('warp-remove-dialog');
  if (removeDialog) document.body.append(removeDialog);
  $('warp-settings-remove')?.addEventListener('click', () => removeDialog?.showModal());
  $('warp-remove-close')?.addEventListener('click', () => removeDialog?.close());
  $('warp-remove-done')?.addEventListener('click', () => removeDialog?.close());
  removeDialog?.addEventListener('click', (event) => {
    if (event.target === removeDialog) removeDialog.close();
  });
  onLangChange(render);
  $('warp-connect').addEventListener('click', () => {
    if (busy && operation === 'connecting') {
      cancelRequested = true; operation = 'disconnecting'; render(); return;
    }
    const disconnect = snapshot?.connected || snapshot?.state === 'connecting';
    act(disconnect ? 'disconnect_warp' : 'connect_warp', undefined, disconnect ? 'disconnecting' : 'connecting');
  });
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
  // A tray query may own the client gate during startup. Do not reveal an
  // undecided layout; wait for detection or let the startup screen offer retry.
  return (async () => {
    const deadline = Date.now() + 45000;
    do {
      await refresh();
      if (snapshot) return;
      if (requestError && requestError.code !== 'warp_busy') throw requestError;
      await new Promise(resolve => window.setTimeout(resolve, 200));
    } while (Date.now() < deadline);
    throw new Error('WARP detection timed out');
  })().finally(() => window.setTimeout(poll, 3000));
}
