import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { setStrategyValue, setPollStatus as setStrategyPollStatus } from './strategies.js';
import { setPollStatus } from '../lib/restart.js';
import { state } from '../lib/state.js';
import { setZapretSummary } from './connection-summary.js';

export function updateStatusUI(status) {
  // During a controlled restart winws/service is briefly absent. Keep the
  // explicit "Restarting" state instead of flashing "Disconnected".
  if (state.restartInProgress) return;
  setZapretSummary(status);

  const trigger = $('strategy-trigger');
  const tempBtn = $('connect-temp-btn');

  if (status.running) {

    const lamp = $('status-lamp-divider');
    if (lamp) {
      lamp.classList.remove('is-off');
      if (!lamp.classList.contains('is-on')) {
        lamp.classList.add('is-on');
      }
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
 
    const lamp = $('status-lamp-divider');
    if (lamp) {
      lamp.classList.remove('is-on');
      if (!lamp.classList.contains('is-off')) {
        lamp.classList.add('is-off');
      }
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
