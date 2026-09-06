import { $ } from '../lib/core.js';
import { t } from '../lib/i18n.js';

let open = false;
let currentOptions = [], currentSelection;
export function closeWarpModes(focus = false) {
  open = false;
  $('warp-mode-options').hidden = true;
  $('warp-mode-trigger').setAttribute('aria-expanded', 'false');
  $('warp-mode-chevron').style.transform = '';
  if (focus) $('warp-mode-trigger').focus({ preventScroll: true });
}

export function renderWarpModes(options, selected, disabled) {
  currentOptions = options; currentSelection = selected;
  const trigger = $('warp-mode-trigger');
  trigger.disabled = disabled;
  $('warp-mode-label').textContent = options.find(([value]) => value === selected)?.[1] || options[0]?.[1] || '';
  if (disabled) closeWarpModes();
  const list = $('warp-mode-options-list');
  const query = $('warp-mode-search').value.trim().toLocaleLowerCase();
  const visibleOptions = options.filter(([, label]) => label.toLocaleLowerCase().includes(query));
  const signature = JSON.stringify([visibleOptions, selected]);
  if (list.dataset.options === signature) return;
  list.dataset.options = signature;
  list.replaceChildren(...visibleOptions.map(([value, label]) => {
    const button = document.createElement('button');
    button.type = 'button';
    button.role = 'option';
    button.dataset.value = value;
    button.setAttribute('aria-selected', String(value === selected));
    button.className = 'group w-full text-left px-4 py-2.5 text-sm font-headline text-on-surface hover:bg-primary/10 transition-colors flex items-center gap-2 cursor-pointer' + (value === selected ? ' dropdown-option-selected' : '');
    button.disabled = !value;
    const icon = document.createElement('span');
    icon.className = 'material-symbols-outlined text-sm';
    icon.setAttribute('aria-hidden', 'true');
    icon.textContent = 'chevron_right';
    icon.style.color = value === selected ? 'var(--color-secondary)' : '';
    icon.style.opacity = value === selected ? '1' : '0.3';
    const text = document.createElement('span');
    text.className = 'truncate flex-1';
    text.textContent = label;
    button.append(icon, text);
    button.addEventListener('click', () => {
      $('warp-mode').value = value;
      closeWarpModes(true);
      $('warp-mode').dispatchEvent(new Event('change'));
    });
    return button;
  }));
  if (!visibleOptions.length) {
    const empty = document.createElement('div');
    empty.className = 'px-4 py-3 text-xs text-on-surface-variant/60 text-center';
    empty.textContent = t('no_results'); list.append(empty);
  }
}

export function initWarpModes() {
  const trigger = $('warp-mode-trigger');
  const list = $('warp-mode-options');
  // Escape the card's stacking context so the popup stays above the app header.
  document.body.append(list);
  function show(last = false, focusOption = false) {
    if (trigger.disabled) return;
    $('warp-mode-search').value = '';
    renderWarpModes(currentOptions, currentSelection, false);
    open = true; list.hidden = false;
    const rect = trigger.getBoundingClientRect();
    list.style.width = `${rect.width}px`;
    list.style.left = `${rect.left}px`;
    const height = list.getBoundingClientRect().height;
    list.style.top = `${rect.bottom + height + 8 > innerHeight ? Math.max(8, rect.top - height - 4) : rect.bottom + 4}px`;
    trigger.setAttribute('aria-expanded', 'true');
    $('warp-mode-chevron').style.transform = 'rotate(180deg)';
    const buttons = [...list.querySelectorAll('button:not(:disabled)')];
    if (focusOption) (last ? buttons.at(-1) : list.querySelector('[aria-selected="true"]') || buttons[0])?.focus({ preventScroll: true });
  }
  trigger.addEventListener('click', () => open ? closeWarpModes() : show());
  $('warp-mode-search').addEventListener('input', () => renderWarpModes(currentOptions, currentSelection, false));
  trigger.addEventListener('keydown', event => {
    if (['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); show(event.key === 'ArrowUp', true); }
  });
  list.addEventListener('keydown', event => {
    const buttons = [...list.querySelectorAll('button:not(:disabled)')];
    const index = buttons.indexOf(document.activeElement);
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
      buttons[next]?.focus({ preventScroll: true });
    }
    if (event.key === 'Tab') closeWarpModes();
  });
  document.addEventListener('keydown', event => { if (open && event.key === 'Escape') { event.preventDefault(); closeWarpModes(true); } });
  document.addEventListener('click', event => { if (open && !$('warp-mode-dropdown').contains(event.target) && !list.contains(event.target)) closeWarpModes(); });
  window.addEventListener('resize', () => closeWarpModes());
  document.addEventListener('scroll', event => { if (open && !list.contains(event.target)) closeWarpModes(); }, true);
}
