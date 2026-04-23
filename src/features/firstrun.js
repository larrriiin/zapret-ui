import { $, invoke, listen } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { loadStrategies } from './strategies.js';
import { runWizard, getSelectedTestType } from './wizard.js';

const FIRSTRUN_KEY = 'zapret.firstrun.dismissed';

function closeFirstrun(dismiss) {
  $('strategies-firstrun-modal')?.classList.add('hidden');
  if (dismiss) localStorage.setItem(FIRSTRUN_KEY, '1');
}

export function initFirstRun() {
  $('firstrun-skip-btn')?.addEventListener('click', () => closeFirstrun(true));

  $('firstrun-download-only-btn')?.addEventListener('click', async () => {
    closeFirstrun(false);
    const modal = $('first-launch-modal');
    const statusEl = $('first-launch-status');
    const progressBar = $('first-launch-progress-bar');
    const progressText = $('first-launch-progress-text');
    if (modal) modal.classList.remove('hidden');
    if (statusEl) statusEl.textContent = t('initializing_download');
    const unlistenProg = await listen('download-progress', (event) => {
      const pct = event.payload;
      if (progressBar) progressBar.style.width = pct + '%';
      if (progressText) progressText.textContent = pct + '%';
      if (statusEl && pct < 90) statusEl.textContent = t('downloading_core');
      if (statusEl && pct >= 90) statusEl.textContent = t('extracting');
    });
    try {
      await invoke('download_and_install_update');
      if (statusEl) statusEl.textContent = t('install_complete');
      if (progressBar) progressBar.style.width = '100%';
      if (progressText) progressText.textContent = '100%';
      await new Promise((r) => setTimeout(r, 1000));
      modal?.classList.add('hidden');
      await loadStrategies();
    } catch (err) {
      if (statusEl) statusEl.textContent = t('download_failed') + ': ' + err;
    } finally {
      try { unlistenProg(); } catch {}
    }
  });

  $('firstrun-download-test-btn')?.addEventListener('click', async () => {
    $('firstrun-download-only-btn')?.click();
    const check = setInterval(async () => {
      try {
        const pre = await invoke('precheck_tests');
        if (pre.strategies_count > 0) {
          clearInterval(check);
          await runWizard(getSelectedTestType());
        }
      } catch {}
    }, 2000);
    setTimeout(() => clearInterval(check), 180000);
  });
}

export async function maybeShowFirstRun() {
  if (localStorage.getItem(FIRSTRUN_KEY) === '1') return;
  try {
    const pre = await invoke('precheck_tests');
    if (pre.strategies_count === 0) {
      $('strategies-firstrun-modal')?.classList.remove('hidden');
    }
  } catch (err) {
    console.warn('precheck_tests failed on startup:', err);
  }
}
