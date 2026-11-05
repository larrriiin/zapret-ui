import { $, listen } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { adminReason } from '../lib/privilege-errors.js';

let reason = 'service', busy = false, previousFocus;
let noticeDismissed = false;
const notices = new Map();
const rawInvoke = (command, args) => window.__TAURI__.core.invoke(command, args);

function render() {
  $('elevation-reason').textContent = t(`elevation_reason_${reason}`);
  $('elevation-restart').disabled = busy;
  $('elevation-cancel').disabled = busy;
  $('elevation-dialog').setAttribute('aria-busy', String(busy));
}

export function requestAdmin(requestedReason = 'service') {
  const dialog = $('elevation-dialog');
  if (!dialog || busy || dialog.open) return;
  reason = requestedReason; previousFocus = document.activeElement;
  $('elevation-error').hidden = true;
  render(); dialog.showModal();
}

export function showAdminNotice(error, source = 'get_application_rules') {
  const requestedReason = adminReason(error);
  if (!$('elevation-notice')) return;
  if (requestedReason) notices.set(source, requestedReason); else notices.delete(source);
  $('elevation-notice').hidden = noticeDismissed || notices.size === 0;
  $('elevation-notice').dataset.reason = notices.values().next().value || 'service';
}

export function initAdminPrivileges() {
  const dialog = $('elevation-dialog');
  $('elevation-cancel').addEventListener('click', () => dialog.close());
  dialog.addEventListener('cancel', event => { if (busy) event.preventDefault(); });
  dialog.addEventListener('close', () => previousFocus?.isConnected && previousFocus.focus({ preventScroll: true }));
  $('elevation-notice-restart').addEventListener('click', () => requestAdmin($('elevation-notice').dataset.reason));
  $('elevation-notice-close').addEventListener('click', () => {
    noticeDismissed = true;
    const restoreFocus = $('elevation-notice').contains(document.activeElement);
    $('elevation-notice').hidden = true;
    if (restoreFocus) document.querySelector('#app-sidebar a.nav-active')?.focus({ preventScroll: true });
  });
  $('elevation-restart').addEventListener('click', async () => {
    if (busy) return;
    busy = true; render(); $('elevation-error').hidden = true;
    try {
      const params = new URLSearchParams(location.search);
      const section = state.currentSectionId === 'zapret-settings'
        ? document.querySelector('[data-zapret-pane][aria-selected="true"]')?.dataset.zapretPane || 'ips'
        : state.currentSectionId || 'home';
      await rawInvoke('restart_as_admin', { context: {
        section: params.get('setup') === '1' ? 'setup' : section,
        repeat: params.get('repeat') === 'true', clear_lists: params.get('clearLists') === 'true',
      } });
    } catch (error) {
      const code = String(error);
      $('elevation-error').textContent = t(code === 'elevation_cancelled' ? 'elevation_cancelled' : code === 'elevation_operation_busy' ? 'elevation_operation_busy' : 'elevation_failed');
      $('elevation-error').hidden = false;
    } finally { busy = false; render(); }
  });
  onLangChange(render);
  listen('admin-required-background', event => showAdminNotice(event.payload)).catch(console.error);
  listen('admin-required-action', event => requestAdmin(event.payload)).catch(console.error);
}
