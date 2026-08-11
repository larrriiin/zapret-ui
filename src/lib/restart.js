import { $, invoke } from './core.js';
import { t } from './i18n.js';
import { state } from './state.js';

// Import lazily to avoid a circular dep with features/status.js.
let pollStatus;
export function setPollStatus(fn) { pollStatus = fn; }

export function beginRestart(message = t('restart_overlay_desc')) {
  state.restartInProgress = true;
  showRestartStatus(t('status_restarting'), true);
  const overlay = $('restart-operation-overlay');
  const messageEl = $('restart-operation-message');
  if (messageEl) messageEl.textContent = message;
  if (overlay) {
    overlay.classList.remove('hidden');
    overlay.classList.add('flex');
  }
}

export function updateRestartOverlay(message) {
  const messageEl = $('restart-operation-message');
  if (messageEl && message) messageEl.textContent = message;
}

export function endRestart() {
  state.restartInProgress = false;
  const overlay = $('restart-operation-overlay');
  if (overlay) {
    overlay.classList.add('hidden');
    overlay.classList.remove('flex');
  }
}

export function showRestartStatus(message, isRestarting = false) {
  const el = $('hero-status');
  if (!el) return;
  el.textContent = message;
  el.classList.remove('animate-status-change');
  void el.offsetWidth; // Force reflow so the CSS animation re-runs.
  el.classList.add('animate-status-change');

  if (isRestarting) {
    el.className = 'text-primary animate-status-change';
    const header = $('header-status');
    if (header) {
      header.innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-primary" data-i18n="status_restarting">${t('status_restarting')}</span>`;
    }
  } else {
    el.className = 'text-secondary animate-status-change';
  }
}

export async function restartServiceIfRunning() {
  const status = await invoke('get_zapret_status');
  if (status.running && status.strategy) {
    beginRestart(t('restart_overlay_desc'));
    try {
      await invoke('stop_zapret');
      await new Promise((r) => setTimeout(r, 1000));
      await invoke('start_zapret', { strategy: status.strategy, mode: status.mode || 'service' });
      showRestartStatus(t('status_connected'));
      endRestart();
      if (pollStatus) {
        await pollStatus();
        setTimeout(() => pollStatus(), 2000);
      }
    } catch (err) {
      console.error('Ошибка перезапуска:', err);
      endRestart();
      showRestartStatus(t('restart_failed') + ': ' + err);
    }
  }
}

export function updateRestartBanner() {
  const banner = $('restart-banner');
  if (!banner) return;
  if (state.pendingRestart) {
    banner.style.display = 'flex';
    banner.classList.remove('opacity-0', 'translate-y-full');
    banner.classList.add('opacity-100', 'translate-y-0');
  } else {
    banner.classList.add('opacity-0', 'translate-y-full');
    banner.classList.remove('opacity-100', 'translate-y-0');
    setTimeout(() => {
      if (!state.pendingRestart) banner.style.display = 'none';
    }, 300);
  }
}

export function showRestartModal() {
  $('restart-modal')?.classList.remove('hidden');
}

export function hideRestartModal() {
  $('restart-modal')?.classList.add('hidden');
}

export async function markRestartIfServiceRunning() {
  try {
    const status = await invoke('get_zapret_status');
    if (status.running) {
      state.pendingRestart = true;
      state.restartGuardDismissed = false;
      updateRestartBanner();
    }
  } catch (err) {
    console.error('get_zapret_status failed:', err);
  }
}
