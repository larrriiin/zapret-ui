import { $, invoke } from '../lib/core.js';

export function initStatusCheck() {
  const checkStatusBtn = $('check-status-btn');
  const statusModal = $('status-modal');
  const statusContent = $('status-content');
  const statusModalClose = $('status-modal-close');

  if (checkStatusBtn && statusModal && statusContent) {
    checkStatusBtn.addEventListener('click', async () => {
      statusContent.textContent = 'Checking status in real-time...';
      statusModal.classList.remove('hidden');
      try {
        const status = await invoke('check_status_full');
        statusContent.textContent = status;
      } catch (err) {
        statusContent.textContent = 'Error checking status: ' + err;
      }
    });
  }

  statusModalClose?.addEventListener('click', () => statusModal?.classList.add('hidden'));
}
