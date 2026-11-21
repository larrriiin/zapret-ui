import { $, invoke } from '../lib/core.js';
import { showConfirm } from '../lib/dom.js';
import { getCurrentLang, t } from '../lib/i18n.js';
import { updateRestartBanner } from '../lib/restart.js';
import { state as appState } from '../lib/state.js';
import { getStrategyValue } from './strategies.js';
import { createWarpDropdown } from './warp-mode.js';
import { makeStatusMessage, makeStatusRow, makeStatusSummary } from './status-check.js';
import { setWireSockSummary } from './connection-summary.js';
import { initWireSockRules } from './wiresock-rules.js';

const PROVIDER_KEY = 'zapret.tunnelProvider';
const LAST_PROFILE_KEY = 'zapret.wiresock.lastConnected';

function stored(key) {
  try { return localStorage.getItem(key) || ''; } catch { return ''; }
}

function store(key, value) {
  try { localStorage.setItem(key, value); } catch { /* Storage is optional. */ }
}

function errorKey(reason, fallback = 'wiresock_cli_failed') {
  const key = typeof reason === 'string' ? reason : reason?.code || reason?.message;
  return key && t(key) !== key ? key : fallback;
}

function profileDate(profile) {
  if (profile.created_at == null) return t('wiresock_created_unknown');
  const date = new Date(profile.created_at * 1000);
  return t('wiresock_created', { date: new Intl.DateTimeFormat(getCurrentLang(), {
    dateStyle: 'short', timeStyle: 'short',
  }).format(date) });
}

function renderMessage(id, key, params) {
  const element = $(id);
  element.hidden = !key;
  element.textContent = key ? t(key, params ?? {}) : '';
}

function initSplitEditor(trigger, onSaved, ruleOptions) {
  const dialog = $('wiresock-split-dialog');
  const form = $('wiresock-split-form');
  const fields = $('wiresock-split-fields');
  const save = $('wiresock-split-save');
  const close = $('wiresock-split-close');
  const toggle = $('wiresock-global-toggle');
  let profile = '';
  let global = null;
  let enabled = false;
  let globalDraft;
  let profileDraft;
  let profileError;
  let profileInitial;
  let saving = false;
  const rules = initWireSockRules(fields, ruleOptions, updateControls);

  function updateControls() {
    const globalChange = global && enabled !== global.enabled;
    const globalSave = enabled || globalChange;
    const current = rules.value();
    fields.hidden = !global || (!enabled && (!profileDraft || globalChange));
    fields.disabled = fields.hidden || saving;
    rules.setDisabled(fields.disabled);
    close.disabled = saving;
    toggle.disabled = !global || saving;
    toggle.dataset.enabled = String(enabled);
    toggle.setAttribute('aria-pressed', String(enabled));
    $('wiresock-global-hint').hidden = !globalSave;
    $('wiresock-split-copy-hint').hidden = Boolean(globalSave) || !profileDraft;
    $('wiresock-split-profile').hidden = enabled || !profile;
    const label = globalSave ? 'wiresock_global_save' : 'wiresock_split_save_copy';
    save.dataset.i18n = label;
    save.textContent = t(label);
    save.disabled = saving || !global || (globalSave
      ? !globalChange && JSON.stringify(current) === JSON.stringify({ applications: global.applications, sites: global.sites })
      : !profileDraft || (!current.sites.length && JSON.stringify(current) === profileInitial));
  }

  toggle.addEventListener('click', () => {
    const draft = rules.value();
    if (enabled) globalDraft = draft;
    else if (profileDraft) profileDraft = { ...draft, sites: profileDraft.sites === null ? null : draft.sites };
    enabled = !enabled;
    rules.reset(enabled ? globalDraft : profileDraft || {});
    renderMessage('wiresock-split-error', !enabled && !global.enabled ? profileError : '');
    updateControls();
  });
  close.addEventListener('click', () => dialog.close());
  dialog.addEventListener('cancel', event => { if (saving) event.preventDefault(); });
  dialog.addEventListener('close', () => trigger.focus());
  form.addEventListener('submit', async event => {
    event.preventDefault();
    if (save.disabled) return;
    const override = enabled || enabled !== global.enabled;
    const draft = enabled || !override ? rules.value() : globalDraft;
    saving = true;
    updateControls();
    renderMessage('wiresock-split-error', '');
    renderMessage('wiresock-split-progress', 'wiresock_split_saving');
    try {
      if (override) {
        await invoke('save_wiresock_global_rules', { rules: { ...draft, enabled } });
      } else {
        const name = await invoke('save_wiresock_split_profile', { profile, ...draft });
        await onSaved(name);
      }
      dialog.close();
    } catch (reason) {
      renderMessage('wiresock-split-error', errorKey(reason, override ? 'wiresock_global_error' : 'wiresock_split_write_error'));
    } finally {
      saving = false;
      renderMessage('wiresock-split-progress', '');
      updateControls();
    }
  });

  let requestId = 0;
  return async name => {
    profile = name;
    const request = ++requestId;
    global = null;
    enabled = false;
    profileDraft = null;
    profileError = '';
    rules.reset();
    $('wiresock-split-profile').textContent = name;
    renderMessage('wiresock-split-error', '');
    renderMessage('wiresock-split-progress', 'wiresock_split_loading');
    updateControls();
    dialog.showModal();
    try {
      const [preferences, result] = await Promise.allSettled([
        invoke('get_wiresock_global_rules'),
        name ? invoke('get_wiresock_split_rules', { profile: name }) : Promise.resolve(null),
      ]);
      if (!dialog.open || request !== requestId) return;
      if (preferences.status === 'rejected') throw preferences.reason;
      global = preferences.value;
      enabled = global.enabled;
      globalDraft = { applications: global.applications, sites: global.sites };
      if (result.status === 'fulfilled') profileDraft = result.value;
      else profileError = errorKey(result.reason, 'wiresock_split_read_error');
      rules.reset(profileDraft || {});
      profileInitial = JSON.stringify(rules.value());
      if (enabled) rules.reset(globalDraft);
      else renderMessage('wiresock-split-error', profileError);
    } catch (reason) {
      if (dialog.open && request === requestId) renderMessage('wiresock-split-error', errorKey(reason, 'wiresock_split_read_error'));
    } finally {
      if (dialog.open && request === requestId) {
        renderMessage('wiresock-split-progress', '');
        updateControls();
      }
    }
  };
}

export function initWireSock(renderHome, openProfile, ruleOptions) {
  const dropdown = createWarpDropdown('wiresock-profile', {
    placeholder: 'wiresock_profile_empty',
    itemAction: { icon: 'delete', label: name => t('wiresock_delete_named', { name }), run: deleteProfile },
  });
  dropdown.init();
  const setupDialog = $('wiresock-setup-dialog');
  const statusDialog = $('wiresock-status-dialog');
  const removeDialog = $('wiresock-remove-dialog');
  document.body.append(removeDialog);
  const profileSelect = $('wiresock-profile');
  const addProfile = $('wiresock-add-profile');
  const addDialog = $('wiresock-add-profile-dialog');
  const splitButton = $('wiresock-split-open');
  const splitToggle = $('wiresock-split-toggle');
  const openSplitEditor = initSplitEditor(splitButton, selectSavedProfile, ruleOptions);
  let preferred = stored(PROVIDER_KEY);
  let lastConnected = stored(LAST_PROFILE_KEY);
  let current = 'warp';
  let status = null;
  let version = null;
  let installing = false;
  let installResult = '';
  let installDetail = '';
  let profiles = [];
  let selected = '';
  let error = '';
  let profileError = '';
  let busy = '';
  let loadingStatus = null;
  let loadingProfiles = null;
  let setupSaving = false;
  let setupRequest;
  let splitTunneling = null;

  const selectedProfile = () => profiles.find(profile => profile.name === selected);
  const tunnelActive = () => status?.state === 'connected' || status?.state === 'connecting';

  function renderStatusReport() {
    if (!statusDialog.open) return;
    const checking = busy === 'refresh';
    const report = $('wiresock-status-report');
    report.setAttribute('aria-busy', String(checking));
    if (checking) {
      report.replaceChildren(makeStatusMessage(t('status_checking_realtime')));
      return;
    }
    const state = status?.error ? 'error' : status?.state || 'error';
    const summary = makeStatusSummary({
      state: state === 'error' ? 'attention' : state === 'connected' ? 'running' : 'stopped',
      title: t(`warp_state_${state}`),
      detail: status?.error ? t(status.error) : undefined,
    });
    summary.id = 'wiresock-status-state';
    report.replaceChildren(summary);
    if (version) {
      report.append(makeStatusRow({ icon: 'shield', labelKey: 'wiresock_version', detail: version, showBadge: false }));
    }
    if (status?.profile) {
      const row = makeStatusRow({ icon: 'route', labelKey: 'wiresock_status_profile', detail: status.profile, showBadge: false });
      row.id = 'wiresock-status-profile';
      report.append(row);
    }
    if (status?.address) {
      const row = makeStatusRow({ icon: 'language', labelKey: 'wiresock_address', detail: status.address, showBadge: false });
      row.id = 'wiresock-address';
      report.append(row);
    }
  }

  function render(warpInstalled, warpBusy) {
    const wireInstalled = status?.installed === true;
    setWireSockSummary(wireInstalled && status.state === 'connected' && !status.error);
    $('wiresock-settings-status').textContent = wireInstalled
      ? t(version ? 'wiresock_installed_version' : 'warp_installed_short', { version })
      : t(status ? 'wiresock_module_missing' : 'warp_detecting');
    $('wiresock-install').hidden = wireInstalled;
    $('wiresock-install').disabled = installing || !status;
    $('wiresock-install').textContent = t(installing ? 'wiresock_installing' : 'wiresock_install');
    $('wiresock-install-result').textContent = [installResult && t(installResult), installDetail].filter(Boolean).join('\n');
    $('wiresock-settings-refresh').hidden = !wireInstalled;
    $('wiresock-settings-refresh').disabled = Boolean(busy || installing);
    $('wiresock-settings-remove').hidden = !wireInstalled;
    current = wireInstalled && (!warpInstalled || preferred === 'wiresock') ? 'wiresock' : 'warp';
    const active = current === 'wiresock';
    $('aux-provider-switch').hidden = !(warpInstalled && wireInstalled);
    $('warp-provider-name').textContent = active ? 'WireSock' : 'Cloudflare';
    $('warp-info-btn').hidden = active;
    $('wiresock-info-btn').hidden = !active;
    $('aux-provider-warp').setAttribute('aria-pressed', String(!active));
    $('aux-provider-wiresock').setAttribute('aria-pressed', String(active));
    $('warp-provider-subtitle').textContent = t(active ? 'wiresock_subtitle' : 'warp_subtitle');
    $('warp-provider-cloud').hidden = active;
    $('wiresock-logo').hidden = !active;
    $('warp-native-panel').hidden = active;
    $('wiresock-panel').hidden = !active;
    $('aux-provider-warp').disabled = Boolean(busy);
    $('aux-provider-wiresock').disabled = warpBusy;
    renderStatusReport();
    if (!active) {
      dropdown.close();
      return;
    }
    $('warp-card-controls').inert = false;
    $('warp-card-initializing').hidden = true;
    const canStop = tunnelActive();
    const transitioning = busy === 'connecting' || busy === 'disconnecting';
    const state = transitioning ? busy : status.state || (status.error ? 'error' : 'detecting');
    $('warp-state').textContent = t(state === 'detecting' ? 'warp_detecting' : `warp_state_${state}`);
    $('warp-card').dataset.state = state;
    const locked = Boolean(busy || warpBusy);
    const profile = selectedProfile();
    $('wiresock-connect-label').textContent = t(transitioning ? `warp_state_${busy}`
      : busy === 'applying' ? 'wiresock_setup_applying'
      : busy === 'setup' ? 'wiresock_setup_checking' : canStop ? 'warp_disconnect' : 'warp_connect');
    $('wiresock-connect').dataset.action = canStop ? 'stop' : 'start';
    $('wiresock-connect').disabled = locked || (!canStop && (!profile || status.state !== 'disconnected'));
    dropdown.render(profiles.map(item => [item.name, item.name, profileDate(item), !item.imported || status.state === 'disconnected']), selected, locked || !profiles.length);
    const names = profiles.map(item => item.name);
    const signature = JSON.stringify(names);
    if (profileSelect.dataset.options !== signature) {
      profileSelect.dataset.options = signature;
      profileSelect.replaceChildren(...names.map(name => new Option(name, name)));
    }
    profileSelect.value = selected;
    addProfile.disabled = locked;
    splitButton.disabled = locked;
    splitToggle.disabled = locked || splitTunneling === null;
    splitToggle.dataset.enabled = String(splitTunneling === true);
    splitToggle.setAttribute('aria-checked', String(splitTunneling === true));
    $('wiresock-refresh').disabled = locked;
    renderMessage('wiresock-error', error || status.error || profileError);
  }

  async function loadStatus() {
    if (loadingStatus) return loadingStatus;
    loadingStatus = (async () => {
      try {
        status = await invoke('get_wiresock_status') || null;
        version = status?.version ?? null;
        splitTunneling = status?.split_tunneling ?? null;
      }
      catch (reason) { if (status?.installed) status = { installed: true, error: errorKey(reason) }; }
      if (!status?.installed) {
        profiles = []; selected = ''; profileError = '';
        if (installResult === 'wiresock_install_done') installResult = '';
      }
      if (status?.state === 'disconnected' && error === 'wiresock_disconnect_failed') error = '';
    })();
    try { await loadingStatus; } finally { loadingStatus = null; renderHome(); }
  }

  async function loadProfiles() {
    if (!status?.installed) return;
    if (loadingProfiles) return loadingProfiles;
    loadingProfiles = (async () => {
      try {
        const list = await invoke('list_wiresock_profiles');
        profiles = Array.isArray(list) ? list : [];
        if (!selectedProfile()) selected = profiles.find(item => item.name === lastConnected)?.name || profiles[0]?.name || '';
        profileError = '';
      } catch { profileError = 'wiresock_profiles_failed'; }
    })();
    try { await loadingProfiles; } finally { loadingProfiles = null; renderHome(); }
  }

  async function action(operation, run, fallback) {
    if (busy) return;
    busy = operation;
    error = '';
    renderHome();
    try { await run(); } catch (reason) { error = errorKey(reason, fallback); }
    finally { busy = ''; renderHome(); }
  }

  function renderSetup(settings) {
    const pending = settings?.restart_pending;
    const key = pending ? 'wiresock_setup_restart_pending' : settings ? 'wiresock_setup_missing' : 'wiresock_setup_read';
    renderMessage('wiresock-setup-status', key, settings);
    $('wiresock-setup-details').hidden = !settings || pending;
    $('wiresock-setup-skip').hidden = Boolean(pending);
    $('wiresock-setup').hidden = !settings;
    $('wiresock-setup').textContent = t(pending ? 'wiresock_setup_retry' : 'wiresock_setup_button');
  }

  async function checkSetup(request) {
    let settings;
    try {
      settings = await invoke('get_wiresock_setup');
      if (settings && !(settings.domains || settings.ipset || settings.hosts || settings.ipset_mode !== 'loaded' || settings.restart_pending)) return 'connect';
    } catch { /* The user can continue after a failed preflight. */ }
    setupRequest = request;
    renderSetup(settings);
    renderMessage('wiresock-setup-error', '');
    setupDialog.returnValue = '';
    return new Promise(resolve => {
      setupDialog.addEventListener('close', () => resolve(setupDialog.returnValue), { once: true });
      setupDialog.showModal();
    });
  }

  async function acceptConnection(report, request) {
    status = report;
    selected = request.profile;
    lastConnected = request.profile;
    store(LAST_PROFILE_KEY, lastConnected);
    if (request.generated || !selectedProfile()) await loadProfiles();
  }

  function connectionRequest(name) {
    const profile = profiles.find(item => item.name === name);
    return { profile: name, generated: Boolean(profile?.file_source) && !profile.imported };
  }

  async function reconnect(name) {
    const request = connectionRequest(name);
    busy = 'connecting';
    renderHome();
    try {
      await acceptConnection(await invoke('reconnect_wiresock', request), request);
    } catch (reason) {
      await loadStatus();
      if (tunnelActive() && profiles.some(item => item.name === status.profile)) selected = status.profile;
      throw errorKey(reason, 'wiresock_connect_failed');
    }
  }

  async function finishSetup(result, request) {
    appState.pendingRestart = false;
    updateRestartBanner();
    if (result.status) await acceptConnection(result.status, request);
    else error = errorKey(result.connection_error, 'wiresock_connect_failed');
  }

  $('wiresock-setup').addEventListener('click', async () => {
    if (setupSaving) return;
    setupSaving = true;
    const request = setupRequest;
    const controls = ['wiresock-setup', 'wiresock-setup-skip', 'wiresock-setup-close'];
    controls.forEach(id => { $(id).disabled = true; });
    renderMessage('wiresock-setup-error', '');
    renderMessage('wiresock-setup-progress', 'wiresock_setup_applying');
    try {
      const result = await invoke('apply_wiresock_setup', { confirmed: true, request }, { wiresock_setup: request });
      await finishSetup(result, request);
      setupDialog.close('connected');
    } catch (reason) {
      renderMessage('wiresock-setup-error', errorKey(reason, 'wiresock_setup_write'));
      try { renderSetup(await invoke('get_wiresock_setup')); }
      catch { /* Keep the original result visible. */ }
    } finally {
      setupSaving = false;
      renderMessage('wiresock-setup-progress', '');
      controls.forEach(id => { $(id).disabled = false; });
    }
  });
  setupDialog.addEventListener('cancel', event => { if (setupSaving) event.preventDefault(); });

  for (const provider of ['warp', 'wiresock']) {
    $(`aux-provider-${provider}`).addEventListener('click', () => {
      preferred = provider;
      store(PROVIDER_KEY, provider);
      renderHome();
      if (current === 'wiresock') void loadProfiles();
    });
  }
  async function selectProfile(name, reload = false) {
    await loadingStatus;
    await loadingProfiles;
    const previous = selected;
    selected = name;
    if (reload) await loadProfiles();
    if (selectedProfile() && tunnelActive() && selected !== (status.profile || previous)) {
      await reconnect(selected);
    }
  }
  profileSelect.addEventListener('change', () => {
    const name = profileSelect.value;
    void action('profile', () => selectProfile(name), 'wiresock_connect_failed');
  });
  function selectSavedProfile(name) {
    return action('profile', () => selectProfile(name, true), 'wiresock_connect_failed');
  }
  addProfile.addEventListener('click', () => { dropdown.close(); addDialog.showModal(); });
  addDialog.addEventListener('close', () => { if (!$('warp-profile-dialog').open) addProfile.focus(); });
  $('wiresock-create').addEventListener('click', () => {
    addDialog.close();
    openProfile(addProfile, file => { void selectSavedProfile(file.replace(/\.conf$/i, '')); });
  });
  splitButton.addEventListener('click', () => {
    void openSplitEditor(selectedProfile()?.name || '');
  });
  splitToggle.addEventListener('click', () => action('split-toggle', async () => {
    await loadingStatus;
    if (splitTunneling === null) return;
    const enabled = !splitTunneling;
    await invoke('set_wiresock_split_tunneling', { enabled });
    splitTunneling = enabled;
    if (tunnelActive()) {
      const name = status.profile || lastConnected || selected;
      if (!name) throw 'wiresock_connect_failed';
      await reconnect(name);
    }
  }, 'wiresock_split_preference_error'));
  $('wiresock-import').addEventListener('click', async () => {
    addDialog.close();
    await action('import', async () => {
      const name = await invoke('import_wiresock_profile');
      if (name) await selectProfile(name, true);
    }, 'wiresock_import_failed');
    addProfile.focus();
  });
  function deleteProfile(name) {
    void action('delete', async () => {
      const profile = profiles.find(item => item.name === name);
      if (!profile) return;
      const confirmation = {
        app_data: 'wiresock_delete_app_data_confirm',
        legacy_downloads: 'wiresock_delete_downloaded_confirm',
      }[profile.file_source] || 'wiresock_delete_confirm';
      const message = t(confirmation, { name });
      if (!await showConfirm(message, t('wiresock_delete'))) return;
      try {
        await invoke('delete_wiresock_profile', { profile: name, fileSource: profile.file_source ?? null });
        if (lastConnected === name) { lastConnected = ''; store(LAST_PROFILE_KEY, ''); }
      } finally { await loadProfiles(); }
    }, 'wiresock_delete_failed');
  }
  $('wiresock-connect').addEventListener('click', () => {
    if (tunnelActive()) {
      void action('disconnecting', async () => {
        status = await invoke('disconnect_wiresock');
      }, 'wiresock_disconnect_failed');
      return;
    }
    const profile = selectedProfile();
    if (!profile) return;
    const request = { ...connectionRequest(profile.name), strategy: getStrategyValue() };
    void action('setup', async () => {
      if (await checkSetup(request) !== 'connect') return;
      busy = 'connecting';
      renderHome();
      const { profile, generated } = request;
      await acceptConnection(await invoke('connect_wiresock', { profile, generated }), request);
    }, 'wiresock_connect_failed');
  });
  const checkStatus = () => action('refresh', async () => {
    statusDialog.showModal();
    renderStatusReport();
    await loadStatus();
  });
  $('wiresock-refresh').addEventListener('click', checkStatus);
  $('wiresock-settings-refresh').addEventListener('click', checkStatus);
  $('wiresock-settings-remove').addEventListener('click', () => removeDialog.showModal());
  removeDialog.addEventListener('click', event => { if (event.target === removeDialog) removeDialog.close(); });
  $('wiresock-install').addEventListener('click', async () => {
    if (installing) return;
    installing = true;
    installResult = '';
    installDetail = '';
    renderHome();
    try {
      await invoke('install_wiresock');
      const result = await invoke('get_wiresock_installation');
      if (!result?.installed) throw 'wiresock_install_incomplete';
      await loadingStatus;
      await loadStatus();
      installResult = 'wiresock_install_done';
      if (current === 'wiresock') await loadProfiles();
    } catch (reason) {
      installResult = errorKey(reason, 'wiresock_install_error');
      installDetail = typeof reason?.detail === 'string' ? reason.detail : '';
    }
    finally { installing = false; renderHome(); }
  });
  async function poll() {
    if (!busy && !installing) {
      const wasInstalled = status?.installed;
      await loadStatus();
      if (current === 'wiresock' && wasInstalled !== status?.installed) await loadProfiles();
    }
    window.setTimeout(poll, 5000);
  }
  void poll();
  return {
    render,
    installed: () => status?.installed === true,
    resumeSetup(request) {
      void action('applying', async () => {
        const result = await invoke('resume_wiresock_setup');
        if (result) await finishSetup(result, request);
      }, 'wiresock_setup_write');
    },
    seedInstallation(installed) {
      if (status === null && installed === true) status = { installed: true };
    },
  };
}
