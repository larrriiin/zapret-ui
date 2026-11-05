// Thin wrappers around the Tauri IPC surface. `withGlobalTauri: true` in
// tauri.conf.json exposes `window.__TAURI__` at runtime, so we lazily reach
// through it instead of importing `@tauri-apps/api` to keep the bundle small
// and match the behaviour the app had before the Vite migration.
import { adminReason } from './privilege-errors.js';
import { requestAdmin, showAdminNotice } from '../features/admin-check.js';
import { t } from './i18n.js';

export async function invoke(cmd, args) {
  try {
    const result = await window.__TAURI__.core.invoke(cmd, args);
    if (cmd === 'get_warp_status' || cmd === 'get_application_rules') {
      const error = [result?.error, result?.applications_error].find(value => adminReason(value));
      showAdminNotice(error, cmd);
    }
    return result;
  } catch (error) {
    const reason = adminReason(error);
    if (reason) {
      if (getCurrentWindow().label === 'tray-menu') {
        await window.__TAURI__.core.invoke('request_admin_from_tray', { reason });
        return;
      }
      requestAdmin(reason);
      throw Object.assign(new Error(t('elevation_required')), { code: 'elevation_required' });
    }
    throw error;
  }
}

export function listen(event, handler) {
  return window.__TAURI__.event.listen(event, handler);
}

export function getCurrentWindow() {
  return window.__TAURI__.window.getCurrentWindow();
}

export function getUpdater() {
  return window.__TAURI__.updater;
}

export function getProcess() {
  return window.__TAURI__.process;
}

export const $ = (id) => document.getElementById(id);
