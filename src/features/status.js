import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { setStrategyValue, setPollStatus as setStrategyPollStatus } from './strategies.js';
import { setPollStatus } from '../lib/restart.js';

export function updateStatusUI(status) {
  const trigger = $('strategy-trigger');
  const tempBtn = $('connect-temp-btn');

  if (status.running) {
    const label = status.strategy ?? t('status_connected');
    const header = $('header-status');
    if (header) {
      header.innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-secondary">${label}</span>`;
    }

    const hero = $('hero-status');
    if (hero) {
      hero.textContent = t('status_connected');
      hero.className = 'text-secondary';
    }

    const btnText = $('connect-btn-text');
    if (btnText) btnText.textContent = t('stop_service');
    const btnIcon = $('connect-btn-icon');
    if (btnIcon) btnIcon.textContent = 'power_settings_new';
    const btn = $('connect-btn');
    if (btn) btn.dataset.action = 'stop';

    if (tempBtn) {
      tempBtn.disabled = true;
      tempBtn.classList.add('hidden');
    }

    if (status.strategy) {
      setStrategyValue(status.strategy, status.strategy);
    }
  } else {
    const header = $('header-status');
    if (header) {
      header.innerHTML = `<span class="text-primary"><span data-i18n="status_label">${t('status_label')}</span>:</span> <span class="text-error-dim" data-i18n="status_disconnected">${t('status_disconnected')}</span>`;
    }

    const hero = $('hero-status');
    if (hero) {
      hero.textContent = t('status_disconnected');
      hero.className = 'text-error-dim';
    }

    const btnText = $('connect-btn-text');
    if (btnText) btnText.textContent = t('run_service');
    const btnIcon = $('connect-btn-icon');
    if (btnIcon) btnIcon.textContent = 'bolt';
    const btn = $('connect-btn');
    if (btn) btn.dataset.action = 'start';

    if (tempBtn) {
      tempBtn.disabled = false;
      tempBtn.classList.remove('hidden');
    }
    if (trigger) trigger.disabled = false;
  }
}

export async function pollStatus() {
  try {
    const status = await invoke('get_zapret_status');
    updateStatusUI(status);
  } catch (err) {
    console.error('Ошибка опроса статуса:', err);
  }
}

setPollStatus(pollStatus);
setStrategyPollStatus(pollStatus);
