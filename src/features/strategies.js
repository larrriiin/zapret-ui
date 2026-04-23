import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { showRestartStatus } from '../lib/restart.js';

// Re-exported so other features (wizard, titlebar search, etc.) can refresh
// the list after mutating favourites, cached test results or strategy files.

let _strategyValue = '';
let _allStrategies = [];
const FAVORITES_KEY = 'zapret.favorites';

// Injected by status.js to avoid a static import cycle. See setPollStatus.
let _pollStatus = null;
export function setPollStatus(fn) { _pollStatus = fn; }

export function setStrategyValue(value, label) {
  _strategyValue = value;
  const lbl = $('strategy-label');
  if (lbl) {
    lbl.textContent = label;
    lbl.classList.remove('text-on-surface/60');
    lbl.classList.add('text-on-surface');
  }
  const sel = $('strategy-select');
  if (sel) sel.value = value;
}

export function getStrategyValue() {
  return _strategyValue;
}

export function closeStrategyDropdown() {
  const panel = $('strategy-options');
  const chevron = $('strategy-chevron');
  if (panel) panel.classList.add('hidden');
  if (chevron) chevron.style.transform = '';
}

export function initStrategyDropdown() {
  const trigger = $('strategy-trigger');
  const panel = $('strategy-options');
  const chevron = $('strategy-chevron');
  if (!trigger || !panel) return;

  trigger.addEventListener('click', (e) => {
    e.stopPropagation();
    const isOpen = !panel.classList.contains('hidden');
    if (isOpen) {
      closeStrategyDropdown();
    } else {
      const rect = trigger.getBoundingClientRect();
      panel.style.top = rect.bottom + 4 + 'px';
      panel.style.left = rect.left + 'px';
      panel.style.width = rect.width + 'px';
      panel.classList.remove('hidden');
      if (chevron) chevron.style.transform = 'rotate(180deg)';
    }
  });
  document.addEventListener('click', (e) => {
    if (!$('strategy-dropdown')?.contains(e.target)) closeStrategyDropdown();
  });

  const search = $('strategy-search');
  if (search) {
    search.addEventListener('input', () => renderStrategyList());
    trigger.addEventListener('click', () => {
      search.value = '';
      renderStrategyList();
    });
  }
}

function normConfigName(s) {
  return String(s || '')
    .replace(/\.bat$/i, '')
    .trim()
    .toLowerCase();
}

export function findCachedResult(configName) {
  const cached = state.cachedTestResults;
  if (!cached || !cached.results) return null;
  const target = normConfigName(configName);
  return cached.results.find((r) => normConfigName(r.config) === target) || null;
}

export function isCachedBest(configName) {
  const cached = state.cachedTestResults;
  if (!cached || !cached.best) return false;
  return normConfigName(cached.best) === normConfigName(configName);
}

export async function loadCachedTestResults() {
  try {
    state.cachedTestResults = await invoke('load_test_results');
  } catch (err) {
    state.cachedTestResults = null;
  }
  return state.cachedTestResults;
}

function getFavorites() {
  try {
    const raw = localStorage.getItem(FAVORITES_KEY);
    if (!raw) return new Set();
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

function toggleFavorite(name) {
  const favs = getFavorites();
  if (favs.has(name)) favs.delete(name);
  else favs.add(name);
  try { localStorage.setItem(FAVORITES_KEY, JSON.stringify([...favs])); } catch {}
  renderStrategyList();
}

export function renderStrategyList() {
  const list = $('strategy-options-list');
  if (!list) return;
  const query = ($('strategy-search')?.value || '').trim().toLowerCase();
  const favs = getFavorites();
  const current = getStrategyValue();

  list.innerHTML = '';

  const filter = (n) => !query || n.toLowerCase().includes(query);
  const favItems = _allStrategies.filter((n) => favs.has(n) && filter(n));
  const restItems = _allStrategies.filter((n) => !favs.has(n) && filter(n));

  if (favItems.length === 0 && restItems.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'px-4 py-3 text-xs text-on-surface-variant/60 text-center';
    empty.textContent = t('no_results') || 'No results';
    list.appendChild(empty);
    return;
  }

  const renderGroup = (items, isFavGroup) => {
    if (items.length === 0) return;
    if (isFavGroup) {
      const hdr = document.createElement('div');
      hdr.className = 'px-4 pt-2 pb-1 text-[9px] font-bold uppercase tracking-widest text-primary/50';
      hdr.textContent = t('favorites') || 'Favorites';
      list.appendChild(hdr);
    } else if (favItems.length > 0) {
      const sep = document.createElement('div');
      sep.className = 'my-1 border-t border-outline-variant/10';
      list.appendChild(sep);
    }
    items.forEach((name) => {
      const cached = findCachedResult(name);
      const isBest = isCachedBest(name);
      const isSelected = name === current;
      const item = document.createElement('div');
      item.dataset.value = name;
      const baseCls = 'group w-full text-left px-4 py-2.5 text-sm font-headline text-on-surface hover:bg-primary/10 transition-colors flex items-center gap-2 cursor-pointer';
      item.className = baseCls + (isBest ? ' border-l-2 border-secondary' : '') + (isSelected ? ' bg-primary/20 text-primary' : '');

      const icon = document.createElement('span');
      icon.className = 'material-symbols-outlined text-sm item-icon';
      icon.style.color = isSelected ? '#ba9eff' : '';
      icon.style.opacity = isSelected ? '1' : '0.3';
      icon.textContent = 'chevron_right';
      item.appendChild(icon);

      const label = document.createElement('span');
      label.className = 'truncate flex-1';
      label.textContent = name;
      item.appendChild(label);

      if (cached) {
        const pingTxt = cached.avg_ping_ms > 0 ? `${cached.avg_ping_ms}${t('ms')}` : '—';
        const total = cached.http_ok + cached.http_error;
        const color = cached.status === 'success' ? 'text-secondary'
          : cached.status === 'partial' ? 'text-primary'
          : 'text-error-dim';
        const badge = document.createElement('span');
        badge.className = `text-[10px] ${color} font-mono`;
        badge.textContent = `${isBest ? '★ ' : ''}HTTP ${cached.http_ok}/${total} · ${pingTxt}`;
        item.appendChild(badge);
      }

      const star = document.createElement('button');
      star.type = 'button';
      star.className = 'ml-1 opacity-60 hover:opacity-100 transition-opacity';
      star.setAttribute('title', t('toggle_favorite') || 'Toggle favorite');
      const starIcon = document.createElement('span');
      starIcon.className = 'material-symbols-outlined text-sm';
      starIcon.textContent = favs.has(name) ? 'star' : 'star_outline';
      starIcon.style.color = favs.has(name) ? '#ffc857' : '';
      star.appendChild(starIcon);
      star.addEventListener('click', (e) => {
        e.stopPropagation();
        toggleFavorite(name);
      });
      item.appendChild(star);

      item.addEventListener('click', () => {
        setStrategyValue(name, name);
        closeStrategyDropdown();
        renderStrategyList();

        invoke('get_zapret_status').then((status) => {
          if (status.running) {
            showRestartStatus(t('switching_strategy'), true);
            invoke('start_zapret', { strategy: name, mode: status.mode || 'service' }).then(() => {
              // Re-polling is scheduled here so the header/hero reflect the
              // newly applied strategy without waiting for the next tick.
              setTimeout(() => _pollStatus && _pollStatus(), 500);
            });
          }
        });
      });
      list.appendChild(item);
    });
  };

  renderGroup(favItems, true);
  renderGroup(restItems, false);
}

export async function loadStrategies() {
  const sel = $('strategy-select');
  try {
    const strategies = await invoke('get_strategies');
    _allStrategies = Array.isArray(strategies) ? strategies : [];
    if (sel) sel.innerHTML = '';

    if (_allStrategies.length === 0) {
      const label = $('strategy-label');
      if (label) label.textContent = t('no_strategies');
      const list = $('strategy-options-list');
      if (list) list.innerHTML = '';
      return;
    }

    _allStrategies.forEach((name) => {
      if (sel) {
        const opt = document.createElement('option');
        opt.value = name;
        opt.textContent = name;
        sel.appendChild(opt);
      }
    });

    const defaultName = _allStrategies.includes('general') ? 'general' : _allStrategies[0];
    setStrategyValue(defaultName, defaultName);
    renderStrategyList();
  } catch (err) {
    console.error('Error loading strategies:', err);
    const label = $('strategy-label');
    if (label) label.textContent = t('error') + ': ' + err;
  }
}
