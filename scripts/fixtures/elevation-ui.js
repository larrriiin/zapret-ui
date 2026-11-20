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
const wireSockProfile = 'Office VPN';
let wireSockConnected = false;
let setupWireSockInstalled = false;
const originalInvoke = window.__TAURI__.core.invoke;
window.__TAURI__.core.invoke = async (command, args) => {
  window.elevationCalls.push({ command, args });
  if (params.has('wiresock-onboarding')) {
    if (command === 'get_wiresock_installation') return { installed: setupWireSockInstalled, version: setupWireSockInstalled ? '3.8.2.1' : null };
    if (command === 'install_wiresock') { setupWireSockInstalled = true; return; }
  }
  if (params.has('wiresock-permission') || params.has('wiresock-resume')) {
    if (command === 'get_tunnel_installations') return { warp: true, wiresock: true };
    if (command === 'get_wiresock_status') return { installed: true, state: wireSockConnected ? 'connected' : 'disconnected' };
    if (command === 'list_wiresock_profiles') return [{ name: wireSockProfile, imported: true }];
    if (command === 'get_wiresock_setup') return { domains: 11, ipset: 17, hosts: 4, ipset_mode: 'loaded' };
    if (command === 'apply_wiresock_setup') throw 'admin_required:hosts';
    if (command === 'elevation_context' && params.has('wiresock-resume')) {
      return { section: 'home', wiresock_setup: { profile: wireSockProfile, generated: false, strategy: 'general' } };
    }
    if (command === 'resume_wiresock_setup') {
      wireSockConnected = true;
      return { status: { installed: true, state: 'connected', profile: wireSockProfile } };
    }
  }
  if (command === 'get_tunnel_installations' && params.has('wiresock-startup')) {
    return { warp: true, wiresock: true };
  }
  if (command === 'get_tunnel_installations' && params.has('slow-warp')) {
    return { warp: false, wiresock: false };
  }
  if (command === 'get_wiresock_status' && params.has('wiresock-startup')) {
    return new Promise(resolve => {
      window.finishWireSockDetection = () => resolve({ installed: true, state: 'disconnected' });
    });
  }
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
