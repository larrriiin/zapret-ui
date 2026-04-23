import { $, invoke, listen } from '../lib/core.js';
import { t } from '../lib/i18n.js';

// Checks for zapret core binaries on first launch. If missing, downloads them
// and reloads the page so the rest of the UI initialises with valid paths.
// Returns `true` when initialisation should continue.
export async function ensureBinariesPresent() {
  try {
    const binariesPresent = await invoke('ensure_binaries_present');
    if (binariesPresent) return true;

    const modal = $('first-launch-modal');
    const statusEl = $('first-launch-status');
    const progressBar = $('first-launch-progress-bar');
    const progressText = $('first-launch-progress-text');

    if (modal) modal.classList.remove('hidden');
    if (statusEl) statusEl.textContent = t('initializing_download');

    listen('download-progress', (event) => {
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
      location.reload();
    } catch (err) {
      if (statusEl) statusEl.textContent = t('download_failed') + ': ' + err + '\n\n' + t('restart_to_fix');
    }
    return false;
  } catch (err) {
    console.error('Failed to check binaries:', err);
    return true;
  }
}
