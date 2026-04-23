// Thin wrappers around the Tauri IPC surface. `withGlobalTauri: true` in
// tauri.conf.json exposes `window.__TAURI__` at runtime, so we lazily reach
// through it instead of importing `@tauri-apps/api` to keep the bundle small
// and match the behaviour the app had before the Vite migration.

export function invoke(cmd, args) {
  return window.__TAURI__.core.invoke(cmd, args);
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
