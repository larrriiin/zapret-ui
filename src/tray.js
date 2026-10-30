import './styles.css';
import './features/tray.css';
import { syncThemeFromStorage } from './features/theme.js';
import { initI18n, t, getCurrentLang, syncTrayLocalization } from './lib/i18n.js';
import { invoke, getCurrentWindow, listen } from './lib/core.js';

const win = getCurrentWindow();
const host = document.getElementById('tray-actions');
let snapshot, selecting = false, refreshing = false, executing = false;
const icons = { show: 'open_in_new', updates: 'system_update', toggle: 'power_settings_new', strategy: 'tune', warp_toggle: 'vpn_lock', quit: 'logout', back: 'arrow_back' };
function row(id, label, disabled = false, action = id) {
  const button = document.createElement('button');
  button.type = 'button'; button.className = 'ui-select-option ui-select-option-sm';
  button.setAttribute('role', 'menuitem'); button.disabled = disabled; button.dataset.action = action;
  const icon = document.createElement('span');
  icon.className = 'material-symbols-outlined'; icon.setAttribute('aria-hidden', 'true');
  icon.textContent = icons[id] || 'chevron_right';
  const text = document.createElement('span'); text.textContent = label;
  if (id === 'toggle' || id === 'warp_toggle') {
    const active = id === 'toggle' ? snapshot.zapret_active : snapshot.warp_active;
    icon.className = 'ui-status-dot'; icon.textContent = ''; icon.dataset.active = String(Boolean(active));
    button.setAttribute('aria-label', `${label}, ${t(active ? 'connected' : 'disconnected')}`);
  }
  button.append(icon, text);
  if (id === 'strategy') {
    text.className = 'tray-strategy-name';
    button.setAttribute('aria-label', `${t('tray_change_strategy')}: ${label}`);
    button.setAttribute('aria-haspopup', 'menu');
    const arrow = document.createElement('span');
    arrow.className = 'material-symbols-outlined tray-strategy-arrow';
    arrow.setAttribute('aria-hidden', 'true'); arrow.textContent = 'chevron_right';
    button.append(arrow);
  }
  button.addEventListener('click', async () => {
    if (executing) return;
    if (id === 'strategy' || id === 'back') {
      selecting = id === 'strategy'; document.activeElement?.blur(); render(); host.focus(); return;
    }
    executing = true;
    host.setAttribute('aria-busy', 'true');
    host.querySelectorAll('button').forEach(item => { item.disabled = true; });
    try { await invoke('tray_action', { action }); }
    catch (error) {
      const output = document.getElementById('tray-error');
      output.textContent = String(error); output.hidden = false;
      await win.show(); await win.setFocus();
    } finally {
      executing = false; host.removeAttribute('aria-busy'); render();
    }
  });
  host.append(button);
}
function separator() {
  const el = document.createElement('div'); el.className = 'tray-separator';
  el.setAttribute('role', 'separator'); host.append(el);
}
function render() {
  const focused = document.activeElement;
  const focusedAction = host.contains(focused) ? focused.dataset.action : null;
  host.replaceChildren();
  if (selecting) {
    row('back', t('tray_show_actions'));
    separator();
    for (const name of snapshot.strategies) row('strategy-item', name, false, `strat_${name}`);
    if (focusedAction) [...host.querySelectorAll('button')].find(b => b.dataset.action === focusedAction)?.focus();
    return;
  }
  row('toggle', snapshot.toggle?.text || t('tray_toggle_on'), !snapshot.toggle?.enabled);
  row('strategy', snapshot.strategy_name || t('tray_change_strategy'), !snapshot.strategies.length);
  separator();
  row('warp_toggle', snapshot.warp_toggle?.text || t('warp_connect'), !snapshot.warp_toggle?.enabled || snapshot.warp_busy);
  separator();
  row('show', t('tray_show')); row('updates', t('updates_view')); row('quit', t('tray_exit'));
  if (focusedAction) [...host.querySelectorAll('button')].find(b => b.dataset.action === focusedAction && !b.disabled)?.focus();
}
async function refresh(force = false) {
  if (refreshing) return;
  refreshing = true;
  try {
    const next = await invoke('tray_snapshot');
    const changed = JSON.stringify(next) !== JSON.stringify(snapshot);
    snapshot = next;
    if (!executing && (force || changed)) { const scroll = host.scrollTop; render(); host.scrollTop = scroll; }
  }
  finally { refreshing = false; }
}
async function open() {
  syncThemeFromStorage(); initI18n(); document.documentElement.lang = getCurrentLang();
  selecting = false; document.activeElement?.blur();
  document.getElementById('tray-error').hidden = true;
  await syncTrayLocalization();
  await refresh(true);
  await win.show(); await win.setFocus();
  host.focus();
}
await listen('tray-opening', () => open().catch(console.error));
window.addEventListener('storage', () => { syncThemeFromStorage(); initI18n(); });
window.addEventListener('contextmenu', event => event.preventDefault());
window.addEventListener('keydown', event => {
  if (event.key === 'Escape') { event.preventDefault(); win.hide(); return; }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Tab'].includes(event.key)) return;
  event.preventDefault();
  const buttons = [...host.querySelectorAll('button:not(:disabled)')];
  const index = buttons.indexOf(document.activeElement);
  const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 :
    index === -1 ? (event.key === 'ArrowUp' || (event.key === 'Tab' && event.shiftKey) ? buttons.length - 1 : 0) :
    (index + (event.key === 'ArrowUp' || (event.key === 'Tab' && event.shiftKey) ? -1 : 1) + buttons.length) % buttons.length;
  buttons[next]?.focus();
});
setInterval(async () => {
  if (document.visibilityState === 'visible') await refresh().catch(console.error);
}, 1500);
