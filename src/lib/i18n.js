import enDict from '../i18n/en.js';
import ruDict from '../i18n/ru.js';
import { $, invoke } from './core.js';

const LANG_KEY = 'zapret_lang';
const dicts = { en: enDict, ru: ruDict };

let currentLang = 'ru';
const changeListeners = new Set();

export function getCurrentLang() {
  return currentLang;
}

export function initI18n() {
  const saved = localStorage.getItem(LANG_KEY);
  if (saved && dicts[saved]) {
    currentLang = saved;
  } else {
    const sysLang = navigator.language || navigator.userLanguage || '';
    currentLang = sysLang.startsWith('ru') ? 'ru' : 'en';
  }
  updatePageTranslations();
}

export function t(key, params = {}) {
  const dict = dicts[currentLang] || dicts.en;
  let translation = dict[key] || key;
  for (const p of Object.keys(params)) {
    translation = translation.replace(`{${p}}`, params[p]);
  }
  return translation;
}

export function updatePageTranslations() {
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    const key = el.dataset.i18n;
    const attr = el.dataset.i18nAttr;
    const translation = t(key);
    if (attr) {
      el.setAttribute(attr, translation);
    } else if (el.tagName === 'INPUT' && el.placeholder) {
      el.placeholder = translation;
    } else {
      el.textContent = translation;
    }
  });
  const langBtn = $('lang-text');
  if (langBtn) langBtn.textContent = currentLang.toUpperCase();
}

export function onLangChange(fn) {
  changeListeners.add(fn);
  return () => changeListeners.delete(fn);
}

export function toggleLanguage() {
  currentLang = currentLang === 'ru' ? 'en' : 'ru';
  localStorage.setItem(LANG_KEY, currentLang);
  updatePageTranslations();
  for (const fn of changeListeners) {
    try { fn(currentLang); } catch (err) { console.error('i18n listener failed:', err); }
  }
  syncTrayLocalization();
}

export async function syncTrayLocalization() {
  try {
    await invoke('update_tray_translations', {
      translations: {
        exit: t('tray_exit'),
        show: t('tray_show'),
        status_prefix: t('tray_status_prefix'),
        strategy_prefix: t('tray_strategy_prefix'),
        toggle_on: t('tray_toggle_on'),
        toggle_off: t('tray_toggle_off'),
        change_strategy: t('tray_change_strategy'),
        minimized_title: t('tray_minimized_title'),
        minimized_body: t('tray_minimized_body'),
        status_on: t('connected'),
        status_off: t('disconnected'),
      },
    });
  } catch (err) {
    console.error('Failed to sync tray localization:', err);
  }
}
