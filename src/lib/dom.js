import { $ } from './core.js';
import { t } from './i18n.js';

export function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

export function cleanAndValidateDomain(domain) {
  let cleaned = String(domain || '').trim().toLowerCase();
  cleaned = cleaned.replace(/^https?:\/\//, '');
  cleaned = cleaned.replace(/^www\./, '');
  cleaned = cleaned.split('/')[0];
  const domainRegex = /^([a-z0-9]+(-[a-z0-9]+)*\.)+[a-z]{2,}$/;
  return domainRegex.test(cleaned) ? cleaned : null;
}

export function validateIP(ip) {
  const cleaned = String(ip || '').trim();
  const ipRegex = /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:\/(?:3[0-2]|[12]?[0-9]))?$/;
  return ipRegex.test(cleaned) ? cleaned : null;
}

export function showConfirm(message, title = null) {
  return new Promise((resolve) => {
    const modal = document.getElementById('confirm-modal');
    const titleEl = document.getElementById('confirm-modal-title');
    const bodyEl = document.getElementById('confirm-modal-body');
    const okBtn = document.getElementById('confirm-modal-ok');
    const cancelBtn = document.getElementById('confirm-modal-cancel');

    if (!modal || !bodyEl || !okBtn || !cancelBtn) {
      resolve(window.confirm(message));
      return;
    }

    if (titleEl) {
      titleEl.textContent = title || t('information');
    }
    bodyEl.textContent = message;

    okBtn.textContent = t('ok');
    cancelBtn.textContent = t('cancel');

    modal.classList.remove('hidden');

    const cleanUp = (result) => {
      modal.classList.add('hidden');
      okBtn.removeEventListener('click', onOk);
      cancelBtn.removeEventListener('click', onCancel);
      resolve(result);
    };

    function onOk() {
      cleanUp(true);
    }

    function onCancel() {
      cleanUp(false);
    }

    okBtn.addEventListener('click', onOk);
    cancelBtn.addEventListener('click', onCancel);
  });
}
