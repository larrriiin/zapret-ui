import { $, invoke, listen, getUpdater } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { updateRestartBanner, markRestartIfServiceRunning } from '../lib/restart.js';
import { pollStatus } from './status.js';
import { refreshCoreVersion } from './versions.js';

let currentUpdateObject = null;

async function downloadAndInstallUIUpdate(event, updateObj) {
  if (!updateObj) return;
  try {
    const btn = event.target;
    btn.disabled = true;
    btn.innerHTML = `<span class="material-symbols-outlined text-[10px] animate-spin">refresh</span> ${t('downloading_installing')}`;
    await updateObj.downloadAndInstall();
    btn.innerHTML = t('update_installed_restarting');
  } catch (err) {
    console.error('UI update failed:', err);
    alert('UI update failed: ' + err);
    const btn = event.target;
    if (btn) {
      btn.disabled = false;
      btn.innerHTML = t('update_now');
    }
  }
}

async function downloadAndInstallCoreUpdate() {
  try {
    const modalTitle = document.querySelector('#update-modal h3');
    if (modalTitle) modalTitle.textContent = t('downloading_installing');
    await invoke('download_and_install_update');
    if (modalTitle) modalTitle.textContent = t('update_installed_restarting');
    setTimeout(() => location.reload(), 1500);
  } catch (err) {
    console.error('Core update failed:', err);
    alert('Core update failed: ' + err);
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
  const coreStatus = data.core.available
    ? `<span class="px-2 py-0.5 bg-secondary/20 text-secondary text-[10px] font-bold rounded-full uppercase">${t('update_available_short')}</span>`
    : `<span class="text-on-surface-variant/50 text-[10px] font-bold uppercase">${t('up_to_date')}</span>`;

  modal.innerHTML = `
    <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-lg w-full shadow-2xl animate-scale-in">
      <div class="flex flex-col items-center">
        <div class="w-16 h-16 bg-primary/10 rounded-2xl flex items-center justify-center mb-6">
          <span class="material-symbols-outlined text-3xl text-primary">system_update_alt</span>
        </div>
        <h3 class="font-headline text-2xl font-black text-on-surface mb-6 uppercase tracking-tight">${t('check_updates')}</h3>

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
              ${data.ui.available ? `<button id="modal-update-ui-btn" class="px-4 py-2 bg-primary/20 hover:bg-primary/30 border border-primary/20 rounded-xl text-[10px] font-black text-primary uppercase transition-all active:scale-95 shadow-lg shadow-primary/5">${t('update_now')}</button>` : ''}
            </div>
          </div>

          <div class="flex items-center justify-between p-4 bg-white/5 rounded-2xl border border-white/5">
            <div class="flex flex-col items-start text-left">
              <span class="text-[10px] font-bold text-secondary/70 uppercase tracking-wider mb-1">${t('zapret_core')}</span>
              <div class="flex items-center gap-2">
                <span class="text-sm font-bold text-on-surface">v${data.core.current}</span>
                ${data.core.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-secondary">v${data.core.latest}</span>` : ''}
              </div>
            </div>
            <div class="flex flex-col items-end gap-3">
              ${coreStatus}
              ${data.core.available ? `<button id="modal-update-core-btn" class="px-4 py-2 bg-secondary/20 hover:bg-secondary/30 border border-secondary/20 rounded-xl text-[10px] font-black text-secondary uppercase transition-all active:scale-95 shadow-lg shadow-secondary/5">${t('update_now')}</button>` : ''}
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
  modal.querySelector('#modal-update-core-btn')?.addEventListener('click', () => downloadAndInstallCoreUpdate());
}

async function checkForUpdates(manual = false) {
  if (!window.__TAURI__) return;
  const { check } = getUpdater();
  const checkUpdatesBtn = $('check-updates-btn');

  if (manual && checkUpdatesBtn) {
    checkUpdatesBtn.disabled = true;
    checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('updating')}`;
  }

  try {
    const uiLocalVersion = await invoke('get_ui_version_cmd');
    const [uiUpdate, coreRemoteVersion, coreLocalVersion] = await Promise.all([
      check().catch((err) => {
        console.warn('UI update check failed (normal in dev):', err);
        return null;
      }),
      invoke('get_remote_core_version').catch((err) => 'Remote Err: ' + err),
      invoke('get_local_version_cmd').catch((err) => 'Local Err: ' + err),
    ]);

    const hasUIUpdate = !!uiUpdate;
    const hasCoreUpdate = coreRemoteVersion !== 'Unknown' && coreLocalVersion !== 'Unknown' && coreRemoteVersion !== coreLocalVersion;

    if (hasUIUpdate || hasCoreUpdate || manual) {
      showDualUpdateModal({
        ui: { available: hasUIUpdate, current: uiLocalVersion, latest: hasUIUpdate ? uiUpdate.version : uiLocalVersion, updateObj: uiUpdate },
        core: { available: hasCoreUpdate, current: coreLocalVersion, latest: coreRemoteVersion },
      }, manual);
    }
  } catch (err) {
    console.error('Error checking for updates:', err);
    if (manual) showDualUpdateModal(null, true);
  } finally {
    if (manual && checkUpdatesBtn) {
      checkUpdatesBtn.disabled = false;
      checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-sm">update</span> <span data-i18n="check_updates">${t('check_updates')}</span>`;
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
      statusEl.textContent = 'Error: ' + err;
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

    let zapretWasRunning = false;
    let zapretStrategy = null;
    let zapretMode = 'service';

    try {
      statusEl.textContent = t('checking_service_status');
      const status = await invoke('get_zapret_status');
      if (status.running) {
        zapretWasRunning = true;
        zapretStrategy = status.strategy;
        zapretMode = status.mode || 'service';
        statusEl.textContent = t('stopping_before_update');
        await invoke('stop_zapret');
      }

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

      const result = await invoke('download_and_install_update');
      if (unlisten) unlisten();
      if (progressBar) progressBar.style.width = '100%';
      if (progressText) progressText.textContent = '100%';
      statusEl.className = 'text-xs text-secondary font-mono mb-3 text-center';

      if (zapretWasRunning && zapretStrategy) {
        statusEl.textContent = t('update_installed_restarting');
        try {
          await invoke('start_zapret', { strategy: zapretStrategy, mode: zapretMode });
          await pollStatus();
          statusEl.textContent = result + ' Zapret restarted successfully.';
        } catch (restartErr) {
          statusEl.textContent = result + ' Warning: failed to restart: ' + restartErr;
          statusEl.className = 'text-xs text-primary font-mono mb-3 text-center';
        }
      } else {
        statusEl.textContent = result;
      }

      await refreshCoreVersion();

      updateNowBtn.textContent = 'Done';
      updateNowBtn.disabled = false;
      updateNowBtn.onclick = () => location.reload();
    } catch (err) {
      statusEl.textContent = 'Error: ' + err;
      statusEl.className = 'mt-4 text-sm text-error-dim';
      if (zapretWasRunning && zapretStrategy) {
        try { await invoke('start_zapret', { strategy: zapretStrategy, mode: zapretMode }); await pollStatus(); } catch {}
      }
      updateNowBtn.disabled = false;
    }
  });
}

export function initUpdates() {
  const checkUpdatesBtn = $('check-updates-btn');
  if (checkUpdatesBtn) checkUpdatesBtn.addEventListener('click', () => checkForUpdates(true));
  setTimeout(() => checkForUpdates(false), 3000);

  initIPSetUpdateButton();
  initLegacyUpdateNowButton();
}
