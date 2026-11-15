import '../screenshot-mock.js';
import '/main.js';
import { syncThemeFromStorage } from '/features/theme.js';

document.getElementById('screenshot-window-style').remove();
const params = new URLSearchParams(location.search);
localStorage.setItem('zapret_theme', params.get('theme') || 'zapret');
localStorage.setItem('zapret_lang', params.get('lang') || 'ru');
syncThemeFromStorage();
window.elevationCalls = [];
window.restartFailure = 'elevation_cancelled';
window.adminPreferenceSaveFailure = false;
window.backgroundPermission = false;
const originalInvoke = window.__TAURI__.core.invoke;
window.__TAURI__.core.invoke = async (command, args) => {
  window.elevationCalls.push({ command, args });
  if (command === 'get_warp_status' && params.has('removed-warp') && window.finishWarpDetection) {
    return { installed: false, state: 'disconnected' };
  }
  if (command === 'get_warp_status' && params.has('removed-warp')) {
    return new Promise(resolve => {
      window.finishWarpDetection = () => resolve({ installed: false, state: 'disconnected' });
    });
  }
  if (command === 'get_warp_status' && params.has('slow-warp') && !window.warpRecovered) {
    if (!window.failWarpDetection) {
      return new Promise((_, reject) => {
        window.failWarpDetection = () => reject({ code: 'warp_cli_error', detail: 'Test failure' });
      });
    }
    throw { code: 'warp_cli_error', detail: 'Test failure' };
  }
  if (command === 'check_admin_privileges') return false;
  if (command === 'get_always_run_as_admin') return localStorage.getItem('mock.alwaysRunAsAdmin') === 'true';
  if (command === 'set_always_run_as_admin') {
    if (window.adminPreferenceSaveFailure) throw 'save failed';
    localStorage.setItem('mock.alwaysRunAsAdmin', String(args.enabled));
    return args.enabled;
  }
  if (command === 'precheck_tests') return { is_admin: false, service_installed: false, service_running: false, winws_running: false, strategies_count: 7 };
  if (command === 'elevation_context') return params.has('resume') ? { section: 'traffic' } : null;
  if (command === 'get_zapret_status') return { running: false, strategy: null, mode: null };
  if (command === 'get_application_rules') return { config: { enabled: false, rules: [] }, active: [], paused: window.backgroundPermission, error: window.backgroundPermission ? 'Zapret: admin_required:service' : null };
  if (command === 'start_zapret' || command === 'stop_zapret') throw 'admin_required:service';
  if (command === 'start_traffic_monitor') throw 'admin_required:traffic';
  if (command === 'update_hosts') throw 'admin_required:hosts';
  if (command === 'restart_as_admin') {
    await new Promise(resolve => setTimeout(resolve, 100));
    if (window.restartFailure) throw window.restartFailure;
    return;
  }
  return originalInvoke(command, args);
};
