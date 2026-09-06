import { $, invoke, listen, getUpdater } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import {
  beginRestart,
  endRestart,
  markRestartIfServiceRunning,
  updateRestartBanner,
  updateRestartOverlay,
} from '../lib/restart.js';
import { pollStatus } from './status.js';
import { refreshCoreVersion } from './versions.js';
import { loadStrategies } from './strategies.js';

let currentUpdateObject = null;

async function withTimeout(promise, timeoutMs) {
  let timeoutId;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timeoutId = setTimeout(() => reject(new Error('Timeout')), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timeoutId);
  }
}

async function downloadAndInstallUIUpdate(event, updateObj) {
  if (!updateObj) return;
  const btn = event.currentTarget;
  try {
    btn.disabled = true;
    btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin" aria-hidden="true">refresh</span><span>${t('downloading_installing')}</span>`;
    await updateObj.downloadAndInstall();
    btn.innerHTML = t('update_installed_restarting');
  } catch (err) {
    console.error('UI update failed:', err);
    alert(`${t('ui_update_failed')}: ${err}`);
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = t('update_now');
    }
  }
}

async function downloadAndInstallCoreUpdate(useProxy = false, customProxy = null) {
  const modal = $('update-modal');
  const modalTitle = $('update-modal-title') || modal?.querySelector('h3');
  const modalPhase = $('update-modal-phase');
  const closeBtn = $('modal-close-btn');
  const updateBtn = $('modal-update-core-btn');
  let restartOverlayVisible = false;
  let unlistenProgress;
  let unlistenPhase;

  const setPhase = (key) => {
    const message = t(key);
    if (modalPhase) modalPhase.textContent = message;
    if (restartOverlayVisible) updateRestartOverlay(message);
  };

  try {
    if (modalTitle) modalTitle.textContent = t('downloading_installing');
    if (closeBtn) closeBtn.disabled = true;
    if (updateBtn) updateBtn.disabled = true;

    unlistenProgress = await listen('download-progress', (event) => {
      const pct = Number(event.payload) || 0;
      if (pct >= 90 && !restartOverlayVisible) setPhase('extracting_installing');
      else if (!restartOverlayVisible) setPhase('downloading_installing');
    });
    unlistenPhase = await listen('core-update-phase', (event) => {
      const phase = event.payload;
      const phaseKeys = {
        stopping: 'stopping_before_update',
        activating: 'core_update_activating',
        updating_ipset: 'core_update_updating_ipset',
        restarting: 'core_update_restarting',
        complete: 'core_update_finishing',
      };
      if (phase === 'stopping' && !restartOverlayVisible) {
        restartOverlayVisible = true;
        beginRestart(t('stopping_before_update'));
      }
      if (phaseKeys[phase]) setPhase(phaseKeys[phase]);
    });

    const result = await invoke('download_and_install_update', { useProxy, customProxy });
    if (restartOverlayVisible) endRestart();
    restartOverlayVisible = false;

    await Promise.all([refreshCoreVersion(), loadStrategies(), pollStatus()]);
    if (modalTitle) modalTitle.textContent = result.status === 'up_to_date'
      ? t('up_to_date')
      : t('core_update_complete_title');
    if (modalPhase) {
      const completionKey = result.status === 'up_to_date'
        ? 'up_to_date_desc'
        : result.restartAttempted && !result.restarted
          ? 'core_update_complete_restart_failed'
          : result.restarted && result.ipsetUpdated
            ? 'core_update_complete_restarted_ipset'
            : result.restarted
              ? 'core_update_complete_restarted'
              : 'core_update_complete_stopped';
      modalPhase.textContent = t(completionKey);
      if (result.warnings?.length) {
        modalPhase.textContent += ` ${t('update_warning')}: ${result.warnings.join('; ')}`;
        modalPhase.classList.add('text-primary');
      }
    }
    if (updateBtn) updateBtn.classList.add('hidden');
    if (closeBtn) {
      closeBtn.disabled = false;
      closeBtn.textContent = t('close');
    }
    return result;
  } catch (err) {
    if (restartOverlayVisible) endRestart();
    restartOverlayVisible = false;
    if (closeBtn) closeBtn.disabled = false;
    if (updateBtn) updateBtn.disabled = false;
    console.error('Core update failed:', err);
    showProxyFallbackModal(err, 
      () => downloadAndInstallCoreUpdate(true),
      (proxy) => downloadAndInstallCoreUpdate(true, proxy),
      () => downloadAndInstallCoreUpdate(false)
    );
    throw err;
  } finally {
    if (unlistenProgress) unlistenProgress();
    if (unlistenPhase) unlistenPhase();
  }
}

function showProxyFallbackModal(errStr, onTryProxy, onCustomProxy, onRetryNoProxy, isCustomProxyStage = false, operation = 'download') {
  const oldModal = $('proxy-fallback-modal');
  if (oldModal) oldModal.remove();

  const modal = document.createElement('div');
  modal.id = 'proxy-fallback-modal';
  modal.className = 'fixed inset-0 z-[2000] flex items-center justify-center p-6 bg-background/80 backdrop-blur-md animate-fade-in';
  
  let content = '';
  if (!isCustomProxyStage) {
    const errorTitle = operation === 'check' ? t('update_check_error_title') : t('download_error_title');
    const errorDescription = operation === 'check' ? t('update_check_error_desc') : t('download_error_desc');
    content = `
      <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-md w-full shadow-2xl animate-scale-in">
        <h3 class="font-headline text-xl font-black text-error mb-4">${errorTitle}</h3>
        <p class="text-on-surface-variant text-sm mb-6">${errorDescription}</p>
        <p data-proxy-error class="text-[10px] text-error-dim font-mono bg-error/10 p-2 rounded mb-6 break-words max-h-32 overflow-y-auto"></p>
        <div class="flex flex-col gap-3">
          <button id="proxy-btn-vpn" class="w-full px-4 py-3 bg-white/5 hover:bg-white/10 text-on-surface rounded-xl font-bold transition-all uppercase text-xs tracking-wider">${t('vpn_enabled')}</button>
          <button id="proxy-btn-proxy" class="w-full px-4 py-3 bg-secondary/20 hover:bg-secondary/30 text-secondary border border-secondary/20 rounded-xl font-black transition-all uppercase text-xs tracking-wider shadow-lg shadow-secondary/5">${t('via_project_proxy')}</button>
          <button id="proxy-btn-close" class="w-full px-4 py-3 mt-2 text-on-surface-variant rounded-xl font-bold hover:bg-white/5 transition-all uppercase text-xs tracking-widest">${t('close')}</button>
        </div>
      </div>
    `;
  } else {
    const proxyErrorDescription = operation === 'check' ? t('update_check_proxy_failed_desc') : t('proxy_failed_desc');
    content = `
      <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-md w-full shadow-2xl animate-scale-in">
        <h3 class="font-headline text-xl font-black text-error mb-4">${t('proxy_failed_title')}</h3>
        <p class="text-on-surface-variant text-sm mb-4">${proxyErrorDescription}</p>
        <input type="text" id="custom-proxy-input" placeholder="socks5://user:pass@ip:port" class="w-full bg-background border border-outline-variant/30 rounded-xl px-4 py-3 text-sm text-on-surface mb-6 focus:outline-none focus:border-primary transition-colors">
        <div class="flex flex-col gap-3">
          <button id="proxy-btn-custom" class="w-full px-4 py-3 bg-secondary/20 hover:bg-secondary/30 text-secondary border border-secondary/20 rounded-xl font-black transition-all uppercase text-xs tracking-wider shadow-lg shadow-secondary/5">${t('download')}</button>
          <button id="proxy-btn-close" class="w-full px-4 py-3 mt-2 text-on-surface-variant rounded-xl font-bold hover:bg-white/5 transition-all uppercase text-xs tracking-widest">${t('close')}</button>
        </div>
      </div>
    `;
  }

  modal.innerHTML = content;
  const errorDetails = modal.querySelector('[data-proxy-error]');
  if (errorDetails) errorDetails.textContent = String(errStr);
  document.body.appendChild(modal);

  modal.querySelector('#proxy-btn-close')?.addEventListener('click', () => modal.remove());
  if (!isCustomProxyStage) {
    modal.querySelector('#proxy-btn-vpn')?.addEventListener('click', () => {
      modal.remove();
      onRetryNoProxy();
    });
    modal.querySelector('#proxy-btn-proxy')?.addEventListener('click', () => {
      const btn = modal.querySelector('#proxy-btn-proxy');
      btn.disabled = true;
      btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('loading')}`;
      onTryProxy().then(() => modal.remove()).catch(err => {
        modal.remove();
        showProxyFallbackModal(err, onTryProxy, onCustomProxy, onRetryNoProxy, true, operation);
      });
    });
  } else {
    modal.querySelector('#proxy-btn-custom')?.addEventListener('click', () => {
      const val = modal.querySelector('#custom-proxy-input').value.trim();
      if (!val) return;
      const btn = modal.querySelector('#proxy-btn-custom');
      btn.disabled = true;
      btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('loading')}`;
      onCustomProxy(val).then(() => modal.remove()).catch(err => {
        btn.disabled = false;
        btn.innerHTML = t('download');
        alert(`${t('error')}: ${err}`);
      });
    });
  }
}

function showDualUpdateModal(data, manual = false) {
  const oldModal = $('update-modal');
  if (oldModal) oldModal.remove();

  if (!data && manual) {
    data = {
      ui: { available: false, current: '...', latest: '...' },
      core: { available: false, current: '...', latest: '...' },
    };
  }

  const modal = document.createElement('div');
  modal.id = 'update-modal';
  modal.className = 'fixed inset-0 z-[1000] flex items-center justify-center p-6 bg-background/80 backdrop-blur-md animate-fade-in';

  const uiStatus = data.ui.available
    ? `<span class="px-2 py-0.5 bg-primary/20 text-primary text-[10px] font-bold rounded-full uppercase">${t('update_available_short')}</span>`
    : `<span class="text-on-surface-variant/50 text-[10px] font-bold uppercase">${t('up_to_date')}</span>`;
  const coreStatusLabels = {
    update_available: t('update_available_short'),
    not_installed: t('core_not_installed'),
    up_to_date: t('up_to_date'),
    ahead: t('core_update_ahead'),
    unknown: t('core_update_unknown'),
  };
  const coreStatus = `<span class="${data.core.available ? 'text-secondary' : 'text-on-surface-variant/70'} text-[10px] font-bold uppercase">${coreStatusLabels[data.core.status] || t('core_update_unknown')}</span>`;
  const coreCurrentVersion = data.core.status === 'not_installed'
    ? t('core_not_installed')
    : data.core.status === 'unknown'
      ? t('core_version_unknown')
      : `v${data.core.current}`;

  modal.innerHTML = `
    <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-lg w-full shadow-2xl animate-scale-in">
      <div class="flex flex-col items-center">
        <div class="w-16 h-16 bg-primary/10 rounded-2xl flex items-center justify-center mb-6">
          <span class="material-symbols-outlined text-3xl text-primary">system_update_alt</span>
        </div>
        <h3 id="update-modal-title" class="font-headline text-2xl font-black text-on-surface mb-2 uppercase tracking-tight">${t('check_updates')}</h3>
        <p id="update-modal-phase" class="min-h-5 text-xs text-on-surface-variant text-center mb-6"></p>

        <div class="w-full space-y-3 mb-8">
          <div class="flex items-center justify-between p-4 bg-white/5 rounded-2xl border border-white/5">
            <div class="flex flex-col items-start text-left">
              <span class="text-[10px] font-bold text-primary/70 uppercase tracking-wider mb-1">${t('app_ui')}</span>
              <div class="flex items-center gap-2">
                <span class="text-sm font-bold text-on-surface">v${data.ui.current}</span>
                ${data.ui.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-primary">v${data.ui.latest}</span>` : ''}
              </div>
            </div>
            <div class="flex flex-col items-end gap-3">
              ${uiStatus}
              ${data.ui.available ? `<button id="modal-update-ui-btn" class="inline-flex items-center justify-center gap-1.5 px-4 py-2 bg-primary/20 hover:bg-primary/30 border border-primary/20 rounded-xl text-[10px] font-black text-primary uppercase transition-all active:scale-95 shadow-lg shadow-primary/5">${t('update_now')}</button>` : ''}
            </div>
          </div>

          <div class="flex items-center justify-between p-4 bg-white/5 rounded-2xl border border-white/5">
            <div class="flex flex-col items-start text-left">
              <span class="text-[10px] font-bold text-secondary/70 uppercase tracking-wider mb-1">${t('zapret_core')} · ${t('core_stable_channel')}</span>
              <div class="flex items-center gap-2">
                <span class="text-sm font-bold text-on-surface">${coreCurrentVersion}</span>
                ${data.core.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-secondary">${data.core.latest === 'Error' ? t('error') : 'v' + data.core.latest}</span>` : ''}
              </div>
            </div>
            <div class="flex flex-col items-end gap-3">
              ${coreStatus}
              ${(data.core.status === 'update_available' || data.core.status === 'not_installed') ? `<button id="modal-update-core-btn" class="px-4 py-2 bg-secondary/20 hover:bg-secondary/30 border border-secondary/20 rounded-xl text-[10px] font-black text-secondary uppercase transition-all active:scale-95 shadow-lg shadow-secondary/5">${t('update_now')}</button>` : ''}
            </div>
          </div>
        </div>

        <button id="modal-close-btn" class="w-full px-4 py-3 bg-white/5 text-on-surface-variant rounded-xl font-bold hover:bg-white/10 transition-all uppercase text-xs tracking-widest">
          ${t('close')}
        </button>
      </div>
    </div>
  `;
  currentUpdateObject = data.ui.updateObj;
  document.body.appendChild(modal);

  modal.querySelector('#modal-close-btn')?.addEventListener('click', () => modal.remove());
  modal.querySelector('#modal-update-ui-btn')?.addEventListener('click', (e) => downloadAndInstallUIUpdate(e, currentUpdateObject));
  modal.querySelector('#modal-update-core-btn')?.addEventListener('click', (event) => {
    event.currentTarget.disabled = true;
    downloadAndInstallCoreUpdate().catch(console.error);
  });
}

export async function checkUIUpdate(useProxy = false, customProxy = null) {
  const { check } = getUpdater();

  if (!useProxy) return withTimeout(check(), 5000);

  const proxyUrl = customProxy || await invoke('get_update_proxy');
  if (!proxyUrl) {
    throw new Error('No update proxy is configured');
  }
  return withTimeout(check({ proxy: proxyUrl }), 8000);
}

export async function checkForUpdates(manual = false, useProxy = false, customProxy = null, promptOnFailure = true) {
  if (!window.__TAURI__) return;
  const checkUpdatesBtn = $('check-updates-btn');

  if (manual && checkUpdatesBtn) {
    checkUpdatesBtn.disabled = true;
    checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-base animate-spin">refresh</span>`;
  }

  try {
    const uiLocalVersion = await invoke('get_ui_version_cmd');
    const [uiUpdate, coreInfo] = await Promise.all([
      checkUIUpdate(useProxy, customProxy),
      invoke('get_core_update_info', { useProxy, customProxy }),
    ]);

    const hasUIUpdate = !!uiUpdate;
    const hasCoreUpdate = coreInfo.status === 'update_available' || coreInfo.status === 'not_installed';
    const showCoreError = manual && coreInfo.status === 'unknown';

    if (hasUIUpdate || hasCoreUpdate || showCoreError || manual) {
      showDualUpdateModal({
        ui: { available: hasUIUpdate, current: uiLocalVersion, latest: hasUIUpdate ? uiUpdate.version : uiLocalVersion, updateObj: uiUpdate },
        core: { available: hasCoreUpdate, current: coreInfo.currentVersion || t('core_not_installed'), latest: coreInfo.stableVersion, status: coreInfo.status, error: showCoreError },
      }, manual);
    }
  } catch (err) {
    console.warn(`Update check failed${useProxy ? ' with proxy' : ' directly'}:`, err);
    if (manual && promptOnFailure) {
      showProxyFallbackModal(
        err,
        () => checkForUpdates(true, true, null, false),
        (proxy) => checkForUpdates(true, true, proxy, false),
        () => checkForUpdates(true, false, null, true),
        false,
        'check',
      );
    } else if (manual) {
      throw err;
    }
  } finally {
    if (manual && checkUpdatesBtn) {
      checkUpdatesBtn.disabled = false;
      checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-base">update</span>`;
    }
  }
}

function initIPSetUpdateButton() {
  const ipsetUpdateBtn = $('ipset-update-btn');
  if (!ipsetUpdateBtn) return;
  ipsetUpdateBtn.addEventListener('click', async () => {
    const statusEl = $('ipset-update-status');
    statusEl.classList.remove('hidden');
    statusEl.textContent = t('updating');
    statusEl.className = 'mt-4 text-sm text-secondary';
    ipsetUpdateBtn.disabled = true;
    try {
      const result = await invoke('update_ipset_list');
      const countMatch = result.match(/\d+/);
      const count = countMatch ? countMatch[0] : '?';
      statusEl.textContent = t('update_success', { count });
      statusEl.className = 'mt-4 text-sm text-secondary';
      await markRestartIfServiceRunning();
    } catch (err) {
      statusEl.textContent = `${t('error')}: ${err}`;
      statusEl.className = 'mt-4 text-sm text-error-dim';
    } finally {
      ipsetUpdateBtn.disabled = false;
    }
  });
}

function initLegacyUpdateNowButton() {
  $('update-now')?.addEventListener('click', async () => {
    const statusEl = $('update-status');
    const updateNowBtn = $('update-now');
    statusEl.classList.remove('hidden');
    statusEl.className = 'mt-4 text-sm text-secondary';
    updateNowBtn.disabled = true;

    try {
      const progressContainer = $('update-status-container');
      const progressText = $('update-progress-text');
      const progressBar = $('update-progress-bar');
      if (progressContainer) {
        progressContainer.classList.remove('hidden');
        statusEl.textContent = t('downloading_installing');
      }

      const unlisten = await listen('download-progress', (event) => {
        const pct = event.payload;
        if (progressBar) progressBar.style.width = pct + '%';
        if (progressText) progressText.textContent = pct + '%';
        if (statusEl && pct >= 90) statusEl.textContent = t('extracting_installing');
      });

      await invoke('download_and_install_update', { useProxy: false, customProxy: null });
      if (unlisten) unlisten();
      if (progressBar) progressBar.style.width = '100%';
      if (progressText) progressText.textContent = '100%';
      statusEl.className = 'text-xs text-secondary font-mono mb-3 text-center';

      statusEl.textContent = t('core_update_complete_title');
      await pollStatus();

      await refreshCoreVersion();

      updateNowBtn.textContent = t('done');
      updateNowBtn.disabled = false;
      updateNowBtn.onclick = () => location.reload();
    } catch (err) {
      statusEl.textContent = `${t('error')}: ${err}`;
      statusEl.className = 'mt-4 text-sm text-error-dim';
      updateNowBtn.disabled = false;

      showProxyFallbackModal(err, 
        () => downloadAndInstallCoreUpdate(true),
        (proxy) => downloadAndInstallCoreUpdate(true, proxy),
        () => downloadAndInstallCoreUpdate(false)
      );
    }
  });
}

async function refreshRollbackState() {
  const current = $('core-current-install-version');
  const previous = $('core-previous-install-version');
  const previousRow = $('core-previous-version-row');
  const button = $('core-rollback-btn');
  if (!current || !previous || !button) return;
  try {
    const installation = await invoke('get_core_installation_state');
    current.textContent = installation.currentVersion || '—';
    previous.textContent = installation.previousVersion || '—';
    previousRow?.classList.toggle('hidden', !installation.previousVersion);
    button.classList.toggle('hidden', !installation.rollbackAvailable);
    button.disabled = !installation.rollbackAvailable;
    button.textContent = t('core_rollback_button', { version: installation.previousVersion || '' });
  } catch (err) {
    console.error('Cannot read core installation state:', err);
    button.disabled = true;
  }
}

function initCoreRollback() {
  const button = $('core-rollback-btn');
  button?.addEventListener('click', async () => {
    const version = $('core-previous-install-version')?.textContent || '';
    if (!window.confirm(t('core_rollback_confirm', { version }))) return;
    button.disabled = true;
    let restartOverlayVisible = false;
    try {
      const status = await invoke('get_zapret_status');
      if (status.running) {
        restartOverlayVisible = true;
        beginRestart(t('core_rollback_restarting'));
      }
      await invoke('rollback_core_update');
      if (restartOverlayVisible) endRestart();
      restartOverlayVisible = false;
      await refreshRollbackState();
      await refreshCoreVersion();
      await loadStrategies();
      await pollStatus();
      alert(t('core_rollback_success'));
    } catch (err) {
      if (restartOverlayVisible) endRestart();
      alert(`${t('core_rollback_title')}: ${err}`);
      await refreshRollbackState();
    }
  });
  refreshRollbackState();
}

export function initUpdates() {
  const checkUpdatesBtn = $('check-updates-btn');
  if (checkUpdatesBtn) checkUpdatesBtn.addEventListener('click', () => checkForUpdates(true));
  setTimeout(() => checkForUpdates(false), 3000);

  initIPSetUpdateButton();
  initLegacyUpdateNowButton();
  initCoreRollback();
}
