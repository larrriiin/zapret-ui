import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { getStrategyValue } from './strategies.js';
import { pollStatus } from './status.js';

export async function handleConnectClick(event) {
  const btn = event.currentTarget;
  const action = btn.dataset.action;
  const mode = btn.dataset.mode || 'service';

  const mainBtn = $('connect-btn');
  const tempBtn = $('connect-temp-btn');
  if (mainBtn) mainBtn.disabled = true;
  if (tempBtn) tempBtn.disabled = true;

  try {
    const hero = $('hero-status');
    if (action === 'start') {
      const strategy = getStrategyValue();
      if (!strategy) return;
      if (hero) {
        hero.textContent = mode === 'service' ? t('starting_service') : t('starting_temp');
        hero.className = 'text-secondary';
      }
      await invoke('start_zapret', { strategy, mode });
      if (hero) hero.textContent = t('service_started');
    } else {
      if (hero) hero.textContent = t('stopping');
      await invoke('stop_zapret');
      if (hero) hero.textContent = t('disconnected');
    }
    await pollStatus();
  } catch (err) {
    console.error('Ошибка действия:', err);
    const hero = $('hero-status');
    if (hero) {
      hero.textContent = `${t('error')}: ${err}`;
      hero.className = 'text-error-dim text-2xl';
    }
    setTimeout(pollStatus, 3000);
  } finally {
    if (mainBtn) mainBtn.disabled = false;
    if (tempBtn) tempBtn.disabled = false;
    await pollStatus();
  }
}

export function initConnectButtons() {
  $('connect-btn')?.addEventListener('click', handleConnectClick);
  $('connect-temp-btn')?.addEventListener('click', handleConnectClick);
}
