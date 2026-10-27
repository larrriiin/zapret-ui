import { invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';

const STORAGE_KEY = 'zapret_notified_updates_v1';
const COMPONENTS = { ui: 'updates_component_ui', core: 'zapret_core', telegram: 'tg_title' };

export function availableUpdateVersions(data) {
  return Object.fromEntries(Object.keys(COMPONENTS)
    .filter(key => data[key]?.available && !data[key]?.error && typeof data[key]?.latest === 'string' && data[key].latest.trim())
    .map(key => [key, data[key].latest]));
}

export function createUpdateNotifier({ read, write, deliver }) {
  let remembered = {};
  try { remembered = JSON.parse(read() || '{}'); } catch { /* Corrupt preferences are recoverable. */ }
  if (!remembered || typeof remembered !== 'object' || Array.isArray(remembered)) remembered = {};
  let modalSignature = '';
  let delivery = Promise.resolve();
  const notify = async data => {
    const actionable = Object.keys(COMPONENTS).filter(key => data[key]?.available);
    if (!actionable.length) return false;
    const available = availableUpdateVersions(data);
    const signature = JSON.stringify(actionable.map(key => [key, data[key].latest, data[key].status]));
    const showNative = Object.entries(available).some(([key, version]) => remembered[key] !== version);
    const showModal = signature !== modalSignature;
    // Visibility is checked even for known versions: a prior Windows notification
    // must not suppress the normal startup modal in a newly opened app session.
    const hidden = await deliver(Object.keys(available), data, { showNative, showModal });
    if (hidden ? !showNative : !showModal) return false;
    if (!hidden) modalSignature = signature;
    remembered = { ...remembered, ...available };
    try { write(JSON.stringify(remembered)); } catch { /* In-memory deduplication still works. */ }
    return true;
  };
  return data => {
    const pending = delivery.then(() => notify(data));
    delivery = pending.catch(() => {});
    return pending;
  };
}

let notifier;
let openUpdates;
export function notifyUpdates(data, open) {
  openUpdates = open;
  notifier ||= createUpdateNotifier({
    read: () => localStorage.getItem(STORAGE_KEY),
    write: value => localStorage.setItem(STORAGE_KEY, value),
    deliver: async (keys, current, { showNative, showModal }) => {
      const native = await invoke('notify_available_updates', {
        title: t('updates_notification_title'),
        body: `${t('updates_notification_body', { components: keys.map(key => t(COMPONENTS[key])).join(', ') })} ${t('updates_tray_hint')}`,
        actionLabel: t('updates_view'),
        showNotification: showNative,
      });
      if (!native && showModal) openUpdates(current);
      return native;
    },
  });
  return notifier(data);
}
