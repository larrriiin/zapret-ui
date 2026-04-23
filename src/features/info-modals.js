import { $ } from '../lib/core.js';
import { t } from '../lib/i18n.js';

let currentInfoType = null;

function getInfoData() {
  return {
    ipset: { title: t('ipset_info_title'), content: t('ipset_info_content') },
    game: { title: t('game_info_title'), content: t('game_info_content') },
    include: { title: t('include_info_title'), content: t('include_info_content') },
    exclude: { title: t('exclude_info_title'), content: t('exclude_info_content') },
    ip_exclude: { title: t('ip_exclude_info_title'), content: t('ip_exclude_info_content') },
  };
}

function showInfo(type) {
  currentInfoType = type;
  const data = getInfoData()[type];
  if (!data) return;
  const title = $('info-title');
  const content = $('info-content');
  const modal = $('info-modal');
  if (title) title.textContent = data.title;
  if (content) content.innerHTML = data.content;
  if (modal) modal.classList.remove('hidden');
}

export function refreshOpenInfoModal() {
  const modal = $('info-modal');
  if (modal && !modal.classList.contains('hidden') && currentInfoType) {
    showInfo(currentInfoType);
  }
}

export function initInfoModals() {
  const infoModal = $('info-modal');
  const infoClose = $('info-modal-close');

  $('ipset-info-btn')?.addEventListener('click', () => showInfo('ipset'));
  $('game-info-btn')?.addEventListener('click', () => showInfo('game'));
  $('include-info-btn')?.addEventListener('click', () => showInfo('include'));
  $('exclude-info-btn')?.addEventListener('click', () => showInfo('exclude'));
  $('ip-exclude-info-btn')?.addEventListener('click', () => showInfo('ip_exclude'));

  infoClose?.addEventListener('click', () => {
    infoModal?.classList.add('hidden');
    currentInfoType = null;
  });
  infoModal?.addEventListener('click', (e) => {
    if (e.target === infoModal) {
      infoModal.classList.add('hidden');
      currentInfoType = null;
    }
  });
}
