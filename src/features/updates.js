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

async function downloadAndInstallCoreUpdate(useProxy = false, customProxy = null) {
  try {
    const modalTitle = document.querySelector('#update-modal h3');
    if (modalTitle) modalTitle.textContent = t('downloading_installing');
    const result = await invoke('download_and_install_update', { useProxy, customProxy });
    if (modalTitle) modalTitle.textContent = result;
    setTimeout(() => location.reload(), 3000);
    return result;
  } catch (err) {
    console.error('Core update failed:', err);
    showProxyFallbackModal(err, 
      () => downloadAndInstallCoreUpdate(true),
      (proxy) => downloadAndInstallCoreUpdate(true, proxy),
      () => downloadAndInstallCoreUpdate(false)
    );
    throw err;
  }
}

function showProxyFallbackModal(errStr, onTryProxy, onCustomProxy, onRetryNoProxy, isCustomProxyStage = false) {
  const oldModal = $('proxy-fallback-modal');
  if (oldModal) oldModal.remove();

  const modal = document.createElement('div');
  modal.id = 'proxy-fallback-modal';
  modal.className = 'fixed inset-0 z-[2000] flex items-center justify-center p-6 bg-background/80 backdrop-blur-md animate-fade-in';
  
  let content = '';
  if (!isCustomProxyStage) {
    content = `
      <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-md w-full shadow-2xl animate-scale-in">
        <h3 class="font-headline text-xl font-black text-error mb-4">Ошибка загрузки</h3>
        <p class="text-on-surface-variant text-sm mb-6">Похоже, мы не можем получить обновление. Попробуйте использовать прокси или включить VPN. Скорее всего, обновление исправит эту ошибку.</p>
        <p class="text-[10px] text-error-dim font-mono bg-error/10 p-2 rounded mb-6 break-words max-h-32 overflow-y-auto">${errStr}</p>
        <div class="flex flex-col gap-3">
          <button id="proxy-btn-vpn" class="w-full px-4 py-3 bg-white/5 hover:bg-white/10 text-on-surface rounded-xl font-bold transition-all uppercase text-xs tracking-wider">Включил VPN</button>
          <button id="proxy-btn-proxy" class="w-full px-4 py-3 bg-secondary/20 hover:bg-secondary/30 text-secondary border border-secondary/20 rounded-xl font-black transition-all uppercase text-xs tracking-wider shadow-lg shadow-secondary/5">Через прокси</button>
          <button id="proxy-btn-close" class="w-full px-4 py-3 mt-2 text-on-surface-variant rounded-xl font-bold hover:bg-white/5 transition-all uppercase text-xs tracking-widest">${t('close') || 'Закрыть'}</button>
        </div>
      </div>
    `;
  } else {
    content = `
      <div class="bg-surface-container-high border border-outline-variant/30 rounded-3xl p-8 max-w-md w-full shadow-2xl animate-scale-in">
        <h3 class="font-headline text-xl font-black text-error mb-4">Прокси не сработал</h3>
        <p class="text-on-surface-variant text-sm mb-4">Наш тестовый прокси не смог загрузить обновление. Вы можете указать свой прокси-сервер (SOCKS5 или HTTP).</p>
        <input type="text" id="custom-proxy-input" placeholder="socks5://user:pass@ip:port" class="w-full bg-background border border-outline-variant/30 rounded-xl px-4 py-3 text-sm text-on-surface mb-6 focus:outline-none focus:border-primary transition-colors">
        <div class="flex flex-col gap-3">
          <button id="proxy-btn-custom" class="w-full px-4 py-3 bg-secondary/20 hover:bg-secondary/30 text-secondary border border-secondary/20 rounded-xl font-black transition-all uppercase text-xs tracking-wider shadow-lg shadow-secondary/5">Скачать</button>
          <button id="proxy-btn-close" class="w-full px-4 py-3 mt-2 text-on-surface-variant rounded-xl font-bold hover:bg-white/5 transition-all uppercase text-xs tracking-widest">${t('close') || 'Закрыть'}</button>
        </div>
      </div>
    `;
  }

  modal.innerHTML = content;
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
      btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Загрузка...`;
      onTryProxy().then(() => modal.remove()).catch(err => {
        modal.remove();
        showProxyFallbackModal(err, onTryProxy, onCustomProxy, onRetryNoProxy, true);
      });
    });
  } else {
    modal.querySelector('#proxy-btn-custom')?.addEventListener('click', () => {
      const val = modal.querySelector('#custom-proxy-input').value.trim();
      if (!val) return;
      const btn = modal.querySelector('#proxy-btn-custom');
      btn.disabled = true;
      btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Загрузка...`;
      onCustomProxy(val).then(() => modal.remove()).catch(err => {
        btn.disabled = false;
        btn.innerHTML = 'Скачать';
        alert('Ошибка: ' + err);
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
              <span class="text-[10px] font-bold text-secondary/70 uppercase tracking-wider mb-1">${t('zapret_core')} · ${t('core_stable_channel')}</span>
              <div class="flex items-center gap-2">
                <span class="text-sm font-bold text-on-surface">v${data.core.current}</span>
                ${data.core.available ? `<span class="material-symbols-outlined text-xs text-on-surface-variant/40">arrow_forward</span> <span class="text-sm font-bold text-secondary">${data.core.latest === 'Error' ? 'Ошибка' : 'v' + data.core.latest}</span>` : ''}
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
  modal.querySelector('#modal-update-core-btn')?.addEventListener('click', (event) => {
    event.currentTarget.disabled = true;
    downloadAndInstallCoreUpdate().catch(console.error);
  });
}

async function checkUIUpdateWithFallback() {
  const { check } = getUpdater();
  const timeout = (ms) => new Promise((_, reject) => setTimeout(() => reject(new Error('Timeout')), ms));
  
  try {
    return await Promise.race([check(), timeout(5000)]);
  } catch (err) {
    console.warn('UI update check failed/timed out without proxy, trying with proxy:', err);
    try {
      const proxyUrl = await invoke('get_update_proxy');
      if (proxyUrl) {
        return await Promise.race([check({ proxy: proxyUrl }), timeout(8000)]);
      }
    } catch (proxyErr) {
      console.error('UI update check failed/timed out with proxy too:', proxyErr);
    }
  }
  return null;
}

async function checkForUpdates(manual = false) {
  if (!window.__TAURI__) return;
  const checkUpdatesBtn = $('check-updates-btn');

  if (manual && checkUpdatesBtn) {
    checkUpdatesBtn.disabled = true;
    checkUpdatesBtn.innerHTML = `<span class="material-symbols-outlined text-base animate-spin">refresh</span>`;
  }

  try {
    const uiLocalVersion = await invoke('get_ui_version_cmd');
    const [uiUpdate, coreInfo] = await Promise.all([
      checkUIUpdateWithFallback(),
      invoke('get_core_update_info', { useProxy: false, customProxy: null }).catch(async (err) => {
        try {
          return await invoke('get_core_update_info', { useProxy: true, customProxy: null });
        } catch {
          return { status: 'unknown', currentVersion: 'Unknown', stableVersion: 'Error', updateAvailable: false };
        }
      }),
    ]);

    const hasUIUpdate = !!uiUpdate;
    const hasCoreUpdate = coreInfo.updateAvailable;
    const showCoreError = manual && coreInfo.status === 'unknown';

    if (hasUIUpdate || hasCoreUpdate || showCoreError || manual) {
      showDualUpdateModal({
        ui: { available: hasUIUpdate, current: uiLocalVersion, latest: hasUIUpdate ? uiUpdate.version : uiLocalVersion, updateObj: uiUpdate },
        core: { available: hasCoreUpdate, current: coreInfo.currentVersion || t('not_installed'), latest: coreInfo.stableVersion, status: coreInfo.status, error: showCoreError },
      }, manual);
    }
  } catch (err) {
    console.error('Error checking for updates:', err);
    if (manual) showDualUpdateModal(null, true);
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

      const result = await invoke('download_and_install_update', { useProxy: false, customProxy: null });
      if (unlisten) unlisten();
      if (progressBar) progressBar.style.width = '100%';
      if (progressText) progressText.textContent = '100%';
      statusEl.className = 'text-xs text-secondary font-mono mb-3 text-center';

      statusEl.textContent = result;
      await pollStatus();

      await refreshCoreVersion();

      updateNowBtn.textContent = 'Done';
      updateNowBtn.disabled = false;
      updateNowBtn.onclick = () => location.reload();
    } catch (err) {
      statusEl.textContent = 'Error: ' + err;
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
    try {
      await invoke('rollback_core_update');
      await refreshRollbackState();
      await refreshCoreVersion();
      alert(t('core_rollback_success'));
    } catch (err) {
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
