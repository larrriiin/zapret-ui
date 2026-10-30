import { $ } from '../lib/core.js';
import { t } from '../lib/i18n.js';

// Pointer sorting also works inside WebView2, which can intercept native HTML drag events.
export function createRuleSorter({ canStart, save, restore }) {
  let drag = null, frame = null;
  const cards = () => [...$('auto-list').children];
  function place() {
    if (!drag) return;
    const host = $('sections-host'), rect = host.getBoundingClientRect();
    const top = Math.max(rect.top, ($('app-header')?.getBoundingClientRect().bottom || 0) + 8);
    if (drag.y < top + 64) host.scrollTop -= Math.min(18, (top + 64 - drag.y) / 4);
    else if (drag.y > rect.bottom - 64) host.scrollTop += Math.min(18, (drag.y - rect.bottom + 64) / 4);
    const target = cards().filter(c => c !== drag.card).find(c => {
      const r = c.getBoundingClientRect(); return drag.y < r.top + r.height / 2;
    });
    if (drag.card.nextElementSibling !== (target || null)) {
      if (target) $('auto-list').insertBefore(drag.card, target);
      else $('auto-list').append(drag.card);
      drag.handle.setPointerCapture(drag.pointer);
    }
    frame = requestAnimationFrame(place);
  }
  async function finish(cancel = false) {
    if (!drag) return;
    const { card, handle, pointer, original } = drag;
    const order = cards().map(c => c.dataset.ruleId);
    drag = null; cancelAnimationFrame(frame);
    card.classList.remove('auto-rule-dragging');
    if (handle.hasPointerCapture(pointer)) handle.releasePointerCapture(pointer);
    if (cancel || order.every((id, i) => id === original[i])) restore(card.dataset.ruleId);
    else await save(order, card.dataset.ruleId);
  }
  function bind(handle, card) {
    handle.addEventListener('pointerdown', event => {
      if (event.button !== 0 || !canStart()) return;
      event.preventDefault(); handle.focus();
      drag = { card, handle, pointer: event.pointerId, original: cards().map(c => c.dataset.ruleId), y: event.clientY };
      handle.setPointerCapture(event.pointerId); card.classList.add('auto-rule-dragging');
      frame = requestAnimationFrame(place);
    });
    handle.addEventListener('pointermove', event => { if (drag) drag.y = event.clientY; });
    handle.addEventListener('pointerup', () => finish());
    handle.addEventListener('pointercancel', () => finish(true));
    handle.addEventListener('lostpointercapture', () => { if (drag && !handle.hasPointerCapture(drag.pointer)) finish(true); });
    handle.addEventListener('keydown', event => {
      if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key) || !canStart()) return;
      event.preventDefault();
      const order = cards().map(c => c.dataset.ruleId), from = order.indexOf(card.dataset.ruleId);
      const to = event.key === 'Home' ? 0 : event.key === 'End' ? order.length - 1 : Math.max(0, Math.min(order.length - 1, from + (event.key === 'ArrowUp' ? -1 : 1)));
      if (from === to) return;
      order.splice(to, 0, order.splice(from, 1)[0]); save(order, card.dataset.ruleId);
    });
  }
  document.addEventListener('keydown', event => { if (drag && event.key === 'Escape') { event.preventDefault(); finish(true); } });
  return { bind, cancel: () => finish(true), get active() { return !!drag; } };
}

export function createRuleMenu() {
  let trigger = null;
  const menu = document.createElement('div');
  menu.id = 'auto-rule-menu'; menu.className = 'ui-select-popup'; menu.role = 'menu'; menu.hidden = true;
  document.body.append(menu);
  function close(focus = false) {
    menu.hidden = true;
    trigger?.setAttribute('aria-expanded', 'false');
    if (focus && trigger?.isConnected) trigger.focus({ preventScroll: true });
    trigger = null;
  }
  function open(button, actions) {
    if (trigger === button) { close(true); return; }
    close(); trigger = button; button.setAttribute('aria-expanded', 'true');
    menu.setAttribute('aria-label', button.getAttribute('aria-label'));
    menu.replaceChildren(...actions.map(({ key, icon, run, danger }) => {
      const item = document.createElement('button'); item.type = 'button'; item.role = 'menuitem';
      item.className = 'ui-select-option' + (danger ? ' ui-tone-danger' : ''); item.dataset.action = key;
      const symbol = document.createElement('span'); symbol.className = 'material-symbols-outlined'; symbol.textContent = icon; symbol.setAttribute('aria-hidden', 'true');
      const label = document.createElement('span'); label.textContent = t(key); item.append(symbol, label);
      item.addEventListener('click', () => { close(true); run(); }); return item;
    }));
    menu.hidden = false;
    const rect = button.getBoundingClientRect(), bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(innerWidth - bounds.width - 8, rect.right - bounds.width))}px`;
    menu.style.top = `${rect.bottom + bounds.height + 8 > innerHeight ? Math.max(8, rect.top - bounds.height - 4) : rect.bottom + 4}px`;
    menu.firstElementChild.focus({ preventScroll: true });
  }
  menu.addEventListener('keydown', event => {
    const items = [...menu.children], i = items.indexOf(document.activeElement);
    if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (i + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[next].focus();
    }
    if (event.key === 'Escape') { event.preventDefault(); close(true); }
    if (event.key === 'Tab') close(true);
  });
  document.addEventListener('pointerdown', e => { if (!menu.hidden && !menu.contains(e.target) && !trigger?.contains(e.target)) close(); });
  document.addEventListener('scroll', e => { if (!menu.contains(e.target)) close(); }, true);
  window.addEventListener('resize', () => close());
  return { open, close, get active() { return !menu.hidden; } };
}
