import { $ } from '../lib/core.js';
import { t } from '../lib/i18n.js';

// Mode and protocol share the same themed popup, placement and keyboard behavior.
export function createWarpDropdown(prefix, { searchable = false, placeholder = 'warp_protocol_select', itemAction } = {}) {
  const node = suffix => $(`${prefix}${suffix ? `-${suffix}` : ''}`);
  let open = false;
  let currentOptions = [], currentSelection;
  function focusOption(button) {
    if (!button) return;
    button.focus({ preventScroll: true });
    if (itemAction) button.scrollIntoView({ block: 'nearest' });
  }
  function close(focus = false) {
    open = false;
    node('options').hidden = true;
    node('trigger').setAttribute('aria-expanded', 'false');
    node('chevron').style.transform = '';
    if (focus) node('trigger').focus({ preventScroll: true });
  }

  function render(options, selected, disabled) {
    currentOptions = options; currentSelection = selected;
    const trigger = node('trigger');
    trigger.disabled = disabled;
    node('label').textContent = options.find(([value]) => value === selected)?.[1] || t(placeholder);
    if (disabled) close();
    const list = node('options-list');
    const query = (node('search')?.value || '').trim().toLocaleLowerCase();
    const visibleOptions = options.filter(([, label]) => label.toLocaleLowerCase().includes(query));
    const signature = JSON.stringify([visibleOptions, selected]);
    if (list.dataset.options === signature) return;
    list.dataset.options = signature;
    list.replaceChildren(...visibleOptions.map(([value, label, detail, actionEnabled]) => {
      const button = document.createElement('button');
      button.type = 'button';
      button.role = itemAction ? 'menuitemradio' : 'option';
      button.dataset.value = value;
      button.setAttribute(itemAction ? 'aria-checked' : 'aria-selected', String(value === selected));
      button.className = 'group ui-select-option' + (value === selected ? ' dropdown-option-selected' : '');
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
      if (detail) {
        text.className = 'min-w-0 flex-1';
        const title = document.createElement('span');
        const meta = document.createElement('span');
        title.className = 'block truncate'; title.textContent = label;
        meta.className = 'block truncate text-xs text-on-surface-variant'; meta.textContent = detail;
        text.replaceChildren(title, meta);
      }
      button.append(icon, text);
      button.addEventListener('click', () => {
        node('').value = value;
        close(true);
        node('').dispatchEvent(new Event('change'));
      });
      if (!itemAction) return button;

      const row = document.createElement('div');
      row.className = 'ui-select-option-group';
      row.role = 'group';
      const action = document.createElement('button');
      action.type = 'button';
      action.role = 'menuitem';
      action.className = 'ui-icon-button ui-icon-button-sm ui-tone-danger';
      action.disabled = !actionEnabled;
      action.title = itemAction.label(label);
      action.setAttribute('aria-label', action.title);
      const actionIcon = document.createElement('span');
      actionIcon.className = 'material-symbols-outlined';
      actionIcon.setAttribute('aria-hidden', 'true');
      actionIcon.textContent = itemAction.icon;
      action.append(actionIcon);
      action.addEventListener('click', () => { close(true); itemAction.run(value); });
      row.append(button, action);
      return row;
    }));
    if (!visibleOptions.length) {
      const empty = document.createElement('div');
      empty.className = 'px-4 py-3 text-xs text-on-surface-variant/60 text-center';
      empty.textContent = t('no_results'); list.append(empty);
    }
  }

  function init(popupHost = document.body) {
    const trigger = node('trigger');
    const list = node('options');
    // Escape card stacking contexts; native dialogs keep their popups inside the modal top layer.
    popupHost.append(list);
    function show(last = false, moveFocus = false) {
      if (trigger.disabled) return;
      if (searchable) node('search').value = '';
      render(currentOptions, currentSelection, false);
      open = true; list.hidden = false;
      const rect = trigger.getBoundingClientRect();
      list.style.width = `${rect.width}px`;
      list.style.left = `${rect.left}px`;
      const scroller = node('options-list');
      scroller.style.maxHeight = '';
      let height = list.getBoundingClientRect().height;
      const above = rect.top - 12;
      const below = innerHeight - rect.bottom - 12;
      let opensAbove = rect.bottom + height + 8 > innerHeight;
      if (itemAction) {
        opensAbove = height > below && above > below;
        const available = opensAbove ? above : below;
        if (height > available) {
          const frame = height - scroller.getBoundingClientRect().height;
          scroller.style.maxHeight = `${Math.max(0, available - frame)}px`;
          height = list.getBoundingClientRect().height;
        }
      }
      list.style.top = `${opensAbove ? Math.max(8, rect.top - height - 4) : rect.bottom + 4}px`;
      trigger.setAttribute('aria-expanded', 'true');
      node('chevron').style.transform = 'rotate(180deg)';
      const buttons = [...list.querySelectorAll('button:not(:disabled)')];
      const selectedOption = list.querySelector('[aria-selected="true"], [aria-checked="true"]');
      if (moveFocus) focusOption(last ? buttons.at(-1) : selectedOption || buttons[0]);
    }
    trigger.addEventListener('click', () => open ? close() : show(false, Boolean(itemAction)));
    node('search')?.addEventListener('input', () => render(currentOptions, currentSelection, false));
    trigger.addEventListener('keydown', event => {
      if (['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); show(event.key === 'ArrowUp', true); }
    });
    list.addEventListener('keydown', event => {
      const buttons = [...list.querySelectorAll('button:not(:disabled)')];
      const index = buttons.indexOf(document.activeElement);
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
        focusOption(buttons[next]);
      }
      if (event.key === 'Tab') close();
    });
    document.addEventListener('keydown', event => { if (open && event.key === 'Escape') { event.preventDefault(); close(true); } });
    document.addEventListener('click', event => { if (open && !node('dropdown').contains(event.target) && !list.contains(event.target)) close(); });
    window.addEventListener('resize', () => close());
    document.addEventListener('scroll', event => { if (open && !list.contains(event.target)) close(); }, true);
  }

  return { init, render, close };
}
const modes = createWarpDropdown('warp-mode', { searchable: true, placeholder: 'warp_unknown' });
export const initWarpModes = modes.init;
export const renderWarpModes = modes.render;
export const closeWarpModes = modes.close;
const protocols = createWarpDropdown('warp-protocol');
export const initWarpProtocols = protocols.init;
export const renderWarpProtocols = protocols.render;
export const closeWarpProtocols = protocols.close;
