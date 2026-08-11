import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';

export function initStatusCheck() {
  const checkStatusBtn = $('check-status-btn');
  const statusModal = $('status-modal');
  const statusContent = $('status-content');
  const statusModalClose = $('status-modal-close');

  if (checkStatusBtn && statusModal && statusContent) {
    checkStatusBtn.addEventListener('click', async () => {
      statusContent.textContent = t('status_checking_realtime');
      statusModal.classList.remove('hidden');
      try {
        const status = await invoke('check_status_full');
        statusContent.textContent = status;
      } catch (err) {
        statusContent.textContent = `${t('status_check_failed')}: ${err}`;
      }
    });
  }

  statusModalClose?.addEventListener('click', () => statusModal?.classList.add('hidden'));
}
