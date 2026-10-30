import { $, invoke } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import markup from '../components/warp.html?raw';
import { initWarpModes, renderWarpModes, closeWarpModes, initWarpProtocols, renderWarpProtocols, closeWarpProtocols } from './warp-mode.js';
import { setWarpSummary } from './connection-summary.js';
import { makeStatusRow } from './status-check.js';
import { showSection } from './navigation.js';
import { showConfirm } from '../lib/dom.js';
import { openWarpTraffic } from './traffic.js';
import { routingState, needsProxyConfirmation, orderedWarpModes } from './warp-state.js';

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
let routingOperation = null;
let healthReport = null;
let healthError = null;
let activeInfo = null;
let diagArchive = '';
let automationNotice = '';
let auxiliaryCommand = '';
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

function renderRuleToggle(id, enabled, disabled) {
  const toggle = $(id);
  if (!toggle) return;
  toggle.disabled = disabled;
  toggle.dataset.enabled = String(enabled);
  toggle.setAttribute('aria-pressed', String(enabled));
}

const WARP_INFO = {
  sites: ['warp_sites_title', 'warp_sites_hint', 'language'],
  apps: ['warp_apps_title', 'warp_apps_scope', 'apps'],
  restore: ['warp_auto_restore', 'warp_restore_info', 'restore'],
  protocol: ['warp_protocol', 'warp_protocol_info', 'tune'],
  address: ['warp_proxy_address', 'warp_proxy_address_info', 'settings_ethernet'],
  port: ['warp_port', 'warp_port_info', 'settings_ethernet'],
  diagnostics: ['warp_diagnostics', 'warp_diagnostics_info', 'diagnosis'],
};
function renderWarpInfo() {
  document.querySelectorAll('[data-warp-info]').forEach(button => {
    const label = `${t('warp_info')}: ${t(WARP_INFO[button.dataset.warpInfo][0])}`;
    button.setAttribute('aria-label', label); button.title = label;
  });
  if (!activeInfo) return;
  const { kind, path } = activeInfo;
  const config = WARP_INFO[kind];
  $('warp-settings-info-title').textContent = config ? t(config[0]) : path.split(/[\\/]/).pop();
  $('warp-settings-info-text').textContent = t(config ? config[1] : 'warp_apps_path_caption');
  $('warp-settings-info-icon').textContent = config ? config[2] : 'description';
  $('warp-settings-info-path').hidden = Boolean(config);
  $('warp-settings-info-path').textContent = config ? '' : path;
}
function openWarpInfo(kind, path = '') {
  activeInfo = { kind, path };
  renderWarpInfo();
  const dialog = $('warp-settings-info-dialog');
  if (!dialog.open) dialog.showModal();
}

function diagnosticErrors() {
  return [...new Set([
    ...(snapshot?.last_errors || []).map(errorText),
    ...(snapshot?.applications?.error ? [snapshot.applications.error] : []),
    ...(snapshot?.connections || []).filter(c => c.state === 'failed' && c.error).slice(-5).map(c => `${c.target}: ${c.error}`),
  ])];
}

function layout(installed) {
  $('nav-warp-settings').hidden = false;
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
  const locked = busy || checking || status === 'connecting' || status === 'disconnecting';
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
  const options = orderedWarpModes(values).map(value => [value, t(WARP_MODE_KEYS[value] || 'warp_unknown')]);
  if (!selected || !values.includes(selected)) options.unshift([selected || '', t('warp_unknown')]);
  const signature = JSON.stringify(options);
  if (mode.dataset.options !== signature) {
    mode.replaceChildren(...options.map(([value, label]) => new Option(label, value)));
    mode.dataset.options = signature;
  }
  mode.value = selected || '';
  mode.disabled = locked || !values.length;
  renderWarpModes(options, selected, mode.disabled);
  $('warp-open-settings').hidden = false;
  const proxy = s?.proxy;
  $('warp-proxy').hidden = !proxy;
  if (proxy) {
    $('warp-proxy-endpoint').textContent = `${proxy.kind || t('warp_unknown')} · ${proxy.address}:${proxy.port ?? t('warp_unknown')}`;
    if (document.activeElement !== $('warp-port') && !busy) $('warp-port').value = proxy.port ?? '';
    $('warp-port-form').hidden = !proxy.port_editable;
  }
  const apps = s?.applications || { paths: [], enabled: false };
  const rulesVerified = !requestError && !s?.error;
  const appsEnabled = rulesVerified && Boolean(apps.runtime_active);
  $('warp-port').disabled = locked || Boolean(s?.sites?.runtime_active) || Boolean(apps.runtime_active);
  $('warp-port-save').disabled = $('warp-port').disabled;
  const sites = s?.sites;
  const siteInput = $('warp-sites-domains');
  if (sites && (!sitesLoaded || !sitesDirty) && document.activeElement !== siteInput) {
    siteInput.value = sites.domains.join('\n');
    sitesLoaded = true;
  }
  const sitesEnabled = rulesVerified && Boolean(sites?.runtime_active);
  $('warp-home-rules').hidden = false;
  siteInput.disabled = busy;
  $('warp-sites-add').disabled = busy || checking;
  $('warp-sites-toggle').disabled = locked || (!sitesEnabled && !siteInput.value.trim());
  $('warp-sites-toggle').textContent = t(sitesEnabled ? 'warp_sites_disable' : 'warp_sites_enable');
  $('warp-sites-toggle').dataset.enabled = String(sitesEnabled);
  $('warp-sites-toggle').setAttribute('aria-pressed', String(sitesEnabled));
  renderRuleToggle('warp-home-sites-toggle', sitesEnabled, $('warp-sites-toggle').disabled);
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
      const remove = document.createElement('button'); remove.type = 'button'; remove.className = 'ui-icon-button'; remove.title = t('warp_sites_remove'); remove.setAttribute('aria-label', `${t('warp_sites_remove')}: ${domain}`);
      remove.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">close</span>'; remove.disabled = locked;
      remove.addEventListener('click', () => act('save_warp_sites', { domains: siteDomains().filter(item => item !== domain) }));
      actions.append(remove); row.append(icon, name, actions); return row;
    }));
  }
  $('warp-page-status').textContent = t(`warp_state_${s?.state || 'disconnected'}`);
  $('warp-page-status').dataset.state = s?.state || 'disconnected';
  $('warp-apps-empty').hidden = apps.paths.length > 0;
  $('warp-apps-add').disabled = locked;
  $('warp-apps-toggle').disabled = locked || (!appsEnabled && !apps.paths.length);
  $('warp-apps-toggle').textContent = t(appsEnabled ? 'warp_apps_disable' : 'warp_apps_enable');
  $('warp-apps-toggle').dataset.enabled = String(appsEnabled);
  $('warp-apps-toggle').setAttribute('aria-pressed', String(appsEnabled));
  renderRuleToggle('warp-home-apps-toggle', appsEnabled, $('warp-apps-toggle').disabled);
  const list = $('warp-apps-list');
  const listSignature = JSON.stringify([apps.paths, locked, apps.enabled, t('warp_apps_remove'), t('warp_apps_path')]);
  if (list.dataset.signature !== listSignature) {
    list.dataset.signature = listSignature;
    list.replaceChildren(...apps.paths.map(path => {
      const row = document.createElement('li'); row.className = 'warp-rule-card';
      const icon = document.createElement('span'); icon.className = 'material-symbols-outlined'; icon.setAttribute('aria-hidden', 'true'); icon.textContent = 'description';
      const name = document.createElement('span'); name.className = 'warp-rule-name'; name.textContent = path.split(/[\\/]/).pop();
      const actions = document.createElement('div'); actions.className = 'warp-card-actions';
      const details = document.createElement('button'); details.type = 'button'; details.className = 'ui-icon-button'; details.title = t('warp_apps_path'); details.setAttribute('aria-label', `${t('warp_apps_path')}: ${name.textContent}`);
      details.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">info</span>'; details.addEventListener('click', () => openWarpInfo('path', path));
      const remove = document.createElement('button'); remove.type = 'button'; remove.className = 'ui-icon-button'; remove.title = t('warp_apps_remove'); remove.setAttribute('aria-label', `${t('warp_apps_remove')}: ${name.textContent}`);
      remove.innerHTML = '<span class="material-symbols-outlined" aria-hidden="true">close</span>'; remove.disabled = locked;
      remove.addEventListener('click', () => act('save_warp_applications', { paths: apps.paths.filter(item => item !== path) }));
      actions.append(details, remove); row.append(icon, name, actions); return row;
    }));
  }
  for (const [kind, rule, ruleError, count] of [
    ['sites', sites, s?.sites_error, domains.length], ['apps', apps, s?.applications_error, apps.paths.length],
  ]) {
    const error = ruleError || requestError || s?.error;
    const state = routingState(rule, { starting: routingOperation === kind, error });
    const node = $(`warp-${kind}-state`);
    node.dataset.state = state;
    const detail = error ? errorText(error) : rule?.error;
    node.textContent = `${t('warp_rule_count', { count })} · ${t(`warp_rule_${state}`)}${detail ? ` — ${detail}` : ''}`;
    $(`warp-${kind}`).dataset.state = state;
  }
  const preferences = s?.automation || {};
  renderRuleToggle('warp-auto-restore', Boolean(preferences.restore_rules), locked);
  const protocols = [['MASQUE', 'MASQUE'], ['WireGuard', 'WireGuard']];
  $('warp-protocol').value = s?.protocol || '';
  $('warp-protocol').disabled = locked || !s?.protocol_editable;
  renderWarpProtocols(protocols, s?.protocol, $('warp-protocol').disabled);
  const protocolHint = $('warp-protocol-hint');
  protocolHint.hidden = Boolean(s?.protocol_editable && s?.protocol);
  protocolHint.textContent = t(s?.protocol_editable ? 'warp_protocol_unverified' : 'warp_protocol_unavailable');
  $('warp-diag-create').disabled = busy || !s?.diag_supported;
  $('warp-diag-create-label').textContent = t(auxiliaryCommand === 'create_warp_diagnostics' ? 'warp_diag_creating' : 'warp_diag_create');
  $('warp-diag-create').title = s?.diag_supported ? '' : t('warp_diag_unavailable');
  $('warp-diag-support').hidden = Boolean(s?.diag_supported);
  $('warp-diag-reveal').hidden = !diagArchive;
  $('warp-diag-reveal').disabled = busy;
  const error = operationError || requestError || s?.error || s?.automation_error;
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
  renderWarpInfo();
  renderReport();
}

function renderReport() {
  if (!$('warp-status-dialog').open) return;
  const report = $('warp-status-report');
  const errorsExpanded = report.querySelector('[data-warp-recent-errors]')?.open;
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
  const rows = [{ icon: 'cloud', labelKey: 'warp_client', value: snapshot?.installed ? 'selected' : 'not_installed', valueLabel: snapshot?.installed ? t('warp_installed_short') : t('warp_not_installed'), detail: snapshot?.installed ? snapshot.version || t('warp_unknown') : undefined }];
  if (snapshot?.installed) {
    rows.push({ icon: 'power_settings_new', labelKey: 'status_label', value: error ? 'unknown' : snapshot.connected ? 'running' : 'stopped', detail: t(`warp_state_${error ? 'error' : snapshot.state}`) },
      { icon: 'route', labelKey: 'warp_mode', value: snapshot.mode ? 'selected' : 'unknown', detail: t(WARP_MODE_KEYS[snapshot.mode] || 'warp_unknown') },
      { icon: 'tune', labelKey: 'warp_protocol', value: snapshot.protocol ? 'selected' : 'unknown', detail: snapshot.protocol || t('warp_not_checked') });
    if (snapshot.proxy) rows.push({ icon: 'settings_ethernet', labelKey: 'warp_mode_proxy',
      value: healthReport?.proxy_available === true ? 'running' : healthReport?.proxy_available === false ? 'stopped' : 'unknown',
      valueLabel: t(healthReport?.proxy_available === true ? 'warp_probe_available' : healthReport?.proxy_available === false ? 'warp_probe_unavailable' : 'warp_not_checked'),
      detail: `${snapshot.proxy.kind || t('warp_unknown')} · ${snapshot.proxy.address}:${snapshot.proxy.port ?? t('warp_unknown')}` });
    const route = healthReport?.route || 'unverified';
    rows.push({ icon: 'network_check', labelKey: 'warp_route_probe', value: route === 'working' ? 'running' : route === 'failed' ? 'stopped' : 'unknown',
      valueLabel: t(route === 'working' ? 'warp_probe_working' : route === 'failed' ? 'warp_probe_failed' : 'warp_not_checked'),
      detail: `${t('warp_check_last_result')} · www.cloudflare.com/cdn-cgi/trace` });
    for (const [rule, labelKey, count] of [[snapshot.sites, 'warp_sites_title', snapshot.sites?.domains?.length || 0], [snapshot.applications, 'warp_apps_title', snapshot.applications?.paths?.length || 0]]) {
      const ruleError = labelKey === 'warp_sites_title' ? snapshot.sites_error : snapshot.applications_error;
      const state = routingState(rule, { error: error || ruleError });
      rows.push({ icon: labelKey === 'warp_sites_title' ? 'language' : 'apps', labelKey,
        value: state === 'active' ? 'running' : state === 'off' ? 'stopped' : state === 'configured' ? 'selected' : 'unknown',
        valueLabel: t(`warp_rule_${state}`), detail: ruleError ? errorText(ruleError) : t('warp_rule_count', { count }) });
    }
  }
  report.append(...rows.map(makeStatusRow));
  for (const problem of [error, healthError, healthReport?.error, snapshot?.automation_error].filter(Boolean)) {
    const message = document.createElement('p'); message.className = 'warp-error'; message.textContent = errorText(problem); report.append(message);
  }
  const failures = diagnosticErrors();
  if (failures.length) {
    const details = document.createElement('details'); details.dataset.warpRecentErrors = ''; details.open = Boolean(errorsExpanded);
    const summary = document.createElement('summary'); summary.className = 'warp-connection-copy'; summary.textContent = t('warp_recent_errors');
    const list = document.createElement('ul'); list.className = 'warp-diagnostic-errors';
    list.replaceChildren(...failures.map(text => { const li = document.createElement('li'); li.textContent = text; return li; }));
    details.append(summary, list); report.append(details);
  }
}

async function checkStatus() {
  if (busy || checking) return;
  closeWarpModes(); closeWarpProtocols();
  checking = true; operationError = null; healthReport = null; healthError = null;
  $('warp-status-dialog').showModal(); render();
  try {
    await polling;
    await refresh({ duringCheck: true });
    if (!requestError && snapshot?.installed) healthReport = await invoke('check_warp_connection');
  } catch (error) { healthError = error; }
  finally { checking = false; render(); }
}

async function refresh({ duringCheck = false } = {}) {
  if (busy || polling || (checking && !duringCheck)) return polling;
  polling = (async () => {
    try {
      snapshot = await invoke('get_warp_status');
      requestError = null;
      const error = snapshot.automation_error || snapshot.sites_error || snapshot.applications_error || snapshot.error;
      if (snapshot.automation?.restore_rules && error) {
        const signature = JSON.stringify(error);
        if (automationNotice !== signature) {
          automationNotice = signature;
          invoke('notify_available_updates', { title: t('warp_page_title'), body: errorText(error), openUpdates: false }).catch(console.warn);
        }
      } else { automationNotice = ''; }
    } catch (error) {
      if (checking || error?.code !== 'warp_busy') requestError = error;
    } finally { render(); }
  })();
  try { await polling; } finally { polling = null; }
}

async function act(command, args, state = null) {
  if (busy || checking) return;
  busy = true; operation = state; operationError = null; render();
  try {
    await polling; snapshot = await invoke(command, args); requestError = null;
    if (command === 'set_warp_sites' || command === 'save_warp_sites') {
      sitesDirty = false;
      $('warp-sites-domains').value = (snapshot.sites?.domains || []).join('\n');
    }
  }
  catch (error) {
    operationError = error;
    try { snapshot = await invoke('get_warp_status'); requestError = null; } catch { /* Preserve the action's error. */ }
  }
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
    busy = false; operation = null; routingOperation = null; render();
  }
}

async function auxiliary(command, accept) {
  if (busy || checking) return;
  busy = true; auxiliaryCommand = command; operationError = null; render();
  $('warp-diag-create').setAttribute('aria-busy', 'true');
  try { await polling; accept(await invoke(command)); }
  catch (error) { operationError = error; }
  finally {
    busy = false; auxiliaryCommand = '';
    $('warp-diag-create').removeAttribute('aria-busy');
    render();
  }
}

async function toggleRule(kind) {
  if (busy || checking) return;
  const rule = kind === 'sites' ? snapshot?.sites : snapshot?.applications;
  const enabled = !(rule?.runtime_active && !requestError && !snapshot?.error);
  let confirmed = false;
  if (enabled && needsProxyConfirmation(snapshot)) {
    confirmed = await showConfirm(t('warp_confirm_proxy'), t('warp_page_title'));
    if (!confirmed || busy) return;
  }
  routingOperation = enabled ? kind : null;
  await act(kind === 'sites' ? 'set_warp_sites' : 'set_warp_applications', {
    ...(kind === 'sites' ? { domains: siteDomains() } : { paths: rule?.paths || [] }), enabled, confirmed,
  });
}

export function initWarp() {
  hero = document.querySelector('#section-home > section');
  zapret = hero.querySelector(':scope > .relative');
  zapret.classList.add('zapret-connection');
  statusHeading = $('hero-status').parentElement;
  zapretHeader = document.createElement('header');
  zapretHeader.className = 'provider-heading';
  zapretHeader.hidden = true;
  zapretHeader.innerHTML = '<span class="material-symbols-outlined zapret-provider-icon" aria-hidden="true">tune</span><div><h3 class="provider-title-row"><span>ZAPRET</span></h3><p data-i18n="warp_zapret_subtitle"></p></div>';
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
  initWarpModes(); initWarpProtocols();
  const warpInfoDialog = $('warp-settings-info-dialog');
  document.body.append(warpInfoDialog);
  $('warp-settings-info-close').addEventListener('click', () => warpInfoDialog.close());
  warpInfoDialog.addEventListener('click', event => { if (event.target === warpInfoDialog) warpInfoDialog.close(); });
  document.querySelectorAll('[data-warp-info]').forEach(button => button.addEventListener('click', () => openWarpInfo(button.dataset.warpInfo)));
  $('warp-open-settings').addEventListener('click', () => showSection('warp-settings'));
  const sitesDialog = $('warp-sites-dialog');
  document.body.append(sitesDialog);
  $('warp-sites-dialog-close').addEventListener('click', () => sitesDialog.close());
  sitesDialog.addEventListener('click', event => { if (event.target === sitesDialog) sitesDialog.close(); });
  $('warp-copy-proxy').addEventListener('click', async () => {
    const proxy = snapshot?.proxy;
    const address = proxy?.address && proxy?.port ? `${proxy.address}:${proxy.port}` : proxy?.address;
    if (!address) return;
    try { await navigator.clipboard.writeText(address); }
    catch { /* Clipboard access is unavailable in some embedded webviews. */ }
  });
  const toggleApplications = () => toggleRule('apps');
  $('warp-apps-toggle').addEventListener('click', toggleApplications);
  $('warp-home-apps-toggle').addEventListener('click', toggleApplications);
  $('warp-apps-add').addEventListener('click', async () => {
    if (busy || checking) return;
    busy = true; operationError = null; render();
    try {
      await polling;
      const paths = await invoke('choose_warp_applications');
      if (paths.length) snapshot = await invoke('save_warp_applications', { paths: [...new Set([...(snapshot?.applications?.paths || []), ...paths])] });
    } catch (error) { operationError = error; }
    finally { busy = false; render(); }
  });
  const saveSiteDomains = domains => act('save_warp_sites', { domains });
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
  $('warp-sites-domains').addEventListener('input', () => { sitesDirty = true; });
  const toggleSites = () => toggleRule('sites');
  $('warp-sites-toggle').addEventListener('click', toggleSites);
  $('warp-home-sites-toggle').addEventListener('click', toggleSites);
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
  $('warp-mode').addEventListener('change', async event => {
    const mode = event.target.value;
    const leavingTunnel = needsProxyConfirmation(snapshot) && !needsProxyConfirmation({ mode });
    const stoppingRules = mode !== 'proxy' && (snapshot?.sites?.runtime_active || snapshot?.applications?.runtime_active);
    const confirmed = leavingTunnel || stoppingRules;
    if (confirmed && !await showConfirm(t(leavingTunnel ? 'warp_confirm_mode_change' : 'warp_mode_rules_confirm'), t('warp_mode'))) { render(); return; }
    act('set_warp_mode', { mode, confirmed });
  });
  $('warp-view-traffic').addEventListener('click', openWarpTraffic);
  $('warp-auto-restore').addEventListener('click', () => act('set_warp_automation', { preferences: {
    restore_rules: !snapshot?.automation?.restore_rules,
  } }));
  $('warp-protocol').addEventListener('change', async event => {
    const protocol = event.target.value;
    const incompatible = protocol === 'WireGuard' && (snapshot?.mode === 'proxy' || snapshot?.sites?.enabled || snapshot?.applications?.enabled);
    if (incompatible && !await showConfirm(t('warp_confirm_wireguard'), t('warp_protocol'))) { render(); return; }
    act('set_warp_protocol', { protocol, confirmed: incompatible });
  });
  $('warp-diag-create').addEventListener('click', async () => {
    if (!await showConfirm(t('warp_diag_privacy'), t('warp_diag_create'))) return;
    $('warp-diag-result').textContent = t('warp_diag_creating');
    auxiliary('create_warp_diagnostics', path => { diagArchive = path; $('warp-diag-result').textContent = path; });
  });
  $('warp-diag-reveal').addEventListener('click', () => auxiliary('reveal_warp_diagnostics', () => {}));
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
