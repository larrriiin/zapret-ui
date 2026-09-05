import { t, onLangChange } from '../lib/i18n.js';

const STORAGE_KEY = 'zapret_theme';
const ACCENT_KEY = 'zapret_accents';
// Each palette pairs the accent with a darker shade and readable button text.
const PALETTES = {
  zapret: {
    violet: ['#ba9eff', '#8455ef', '#39008c'],
    cyan: ['#73dded', '#3594a4', '#09343c'],
    blue: ['#94baff', '#5b86cc', '#142c52'],
    mint: ['#87ddbd', '#469d80', '#133c30'],
  },
  graphite: {
    steel: ['#b8c7e0', '#8199bb', '#172334'],
    sage: ['#b1cbb0', '#7c9c7b', '#233523'],
    sand: ['#dec69f', '#ac8d5f', '#3c2e19'],
    lavender: ['#c8b8df', '#9983b7', '#30223f'],
  },
  light: {
    violet: ['#6539b7', '#54299f', '#ffffff'],
    blue: ['#245bb5', '#19488f', '#ffffff'],
    teal: ['#086d60', '#075b51', '#ffffff'],
    plum: ['#873c79', '#6b2f60', '#ffffff'],
  },
};
const THEMES = ['zapret', 'graphite', 'light', 'system'];
const systemScheme = window.matchMedia('(prefers-color-scheme: dark)');
let preference = 'zapret';
const accents = {};

try {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (THEMES.includes(saved)) preference = saved;
} catch { /* Keep the default when storage is unavailable. */ }
try {
  const saved = JSON.parse(localStorage.getItem(ACCENT_KEY));
  for (const theme of Object.keys(PALETTES)) {
    if (saved && Object.hasOwn(PALETTES[theme], saved[theme])) accents[theme] = saved[theme];
  }
} catch { /* Ignore malformed or unavailable storage. */ }

function selectedAccent(theme) {
  return accents[theme] || Object.keys(PALETTES[theme])[0];
}

function renderAccents(theme) {
  const container = document.getElementById('accent-options');
  if (!container) return;
  if (container.dataset.theme !== theme) {
    container.replaceChildren();
    container.dataset.theme = theme;
    for (const [name, colors] of Object.entries(PALETTES[theme])) {
      const label = document.createElement('label');
      label.className = 'accent-choice';
      const radio = document.createElement('input');
      radio.type = 'radio';
      radio.name = 'accent-pref';
      radio.value = name;
      const swatch = document.createElement('span');
      swatch.className = 'accent-swatch';
      swatch.style.backgroundColor = colors[0];
      swatch.setAttribute('aria-hidden', 'true');
      const text = document.createElement('span');
      text.className = 'accent-label';
      text.dataset.i18n = `accent_${name}`;
      text.textContent = t(`accent_${name}`);
      label.append(radio, swatch, text);
      container.append(label);
      radio.addEventListener('change', () => {
        if (!radio.checked) return;
        accents[theme] = name;
        try { localStorage.setItem(ACCENT_KEY, JSON.stringify(accents)); } catch { /* Session-only choice. */ }
        applyTheme();
      });
    }
  }
  container.querySelectorAll('input').forEach(input => { input.checked = input.value === selectedAccent(theme); });
}

function applyTheme() {
  const resolved = preference === 'system' ? (systemScheme.matches ? 'zapret' : 'light') : preference;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.classList.toggle('dark', resolved !== 'light');
  document.documentElement.style.colorScheme = resolved === 'light' ? 'light' : 'dark';
  const accent = selectedAccent(resolved);
  document.documentElement.dataset.accent = accent;
  ['primary', 'primary-dim', 'on-primary'].forEach((token, index) => {
    document.documentElement.style.setProperty(`--color-${token}`, PALETTES[resolved][accent][index]);
  });
  renderAccents(resolved);
  document.querySelectorAll('.theme-preview').forEach(preview => {
    const theme = preview.dataset.preview === 'system' ? (systemScheme.matches ? 'zapret' : 'light') : preview.dataset.preview;
    preview.style.setProperty('--preview-accent', PALETTES[theme][selectedAccent(theme)][0]);
  });
  document.querySelectorAll('input[name="theme-pref"]').forEach(input => {
    input.checked = input.value === preference;
  });
  document.dispatchEvent(new CustomEvent('zapret:theme-changed'));
}

// Apply during module loading, before native startup checks and component setup.
applyTheme();
systemScheme.addEventListener('change', () => {
  if (preference === 'system') applyTheme();
});

export function initTheme() {
  applyTheme();
  onLangChange(() => {
    document.querySelectorAll('#accent-options [data-i18n]').forEach(label => { label.textContent = t(label.dataset.i18n); });
  });
  document.querySelectorAll('input[name="theme-pref"]').forEach(input => {
    input.addEventListener('change', () => {
      if (!input.checked || !THEMES.includes(input.value)) return;
      preference = input.value;
      try { localStorage.setItem(STORAGE_KEY, preference); } catch { /* Session-only choice. */ }
      applyTheme();
    });
  });
}

export function syncThemeFromStorage() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (THEMES.includes(saved)) preference = saved;
    const savedAccents = JSON.parse(localStorage.getItem(ACCENT_KEY));
    for (const theme of Object.keys(PALETTES)) {
      delete accents[theme];
      if (savedAccents && Object.hasOwn(PALETTES[theme], savedAccents[theme])) accents[theme] = savedAccents[theme];
    }
  } catch { /* Keep the in-memory preference if storage is unavailable. */ }
  applyTheme();
}
