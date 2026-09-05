import { $, invoke, listen, getCurrentWindow } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { state } from '../lib/state.js';
import { loadStrategies, loadCachedTestResults, setStrategyValue } from './strategies.js';
import { pollStatus } from './status.js';
import { pollFilters } from './filters.js';
import { pollFakes } from './fakes.js';
import { updateRestartBanner } from '../lib/restart.js';
import { syncThemeFromStorage } from './theme.js';

const isSetupWindow = new URLSearchParams(location.search).get('setup') === '1';

const COMPLETE_KEY = 'zapret.setup.completed';
const USER_LISTS = ['list-general-user.txt', 'list-exclude-user.txt', 'ipset-exclude-user.txt'];
let step = 0, busy = false, testStarted = false, cancelled = false;
let prepared = false, best = null, connected = false, rerun = false, deleteLists = false;
let appearanceHome, appearance, resolveSetup, previousFocus, transition;

function errorMessage(error) {
  $('setup-error').textContent = error instanceof Error ? error.message : t(String(error));
  $('setup-error').hidden = false;
}
function controls() {
  $('setup-close').disabled = busy;
  $('setup-minimize').disabled = busy;
  $('setup-back').hidden = step === 0 || step === 3;
  $('setup-back').disabled = busy;
  $('setup-skip').hidden = step !== 2;
  $('setup-skip').disabled = busy && (!testStarted || cancelled);
  $('setup-skip').textContent = t(busy ? 'setup_cancel_test' : best ? 'setup_without_apply' : 'setup_skip');
  $('setup-next').disabled = busy;
  $('setup-next').textContent = t(['setup_continue', 'setup_prepare_action', best ? 'setup_apply' : 'setup_test_action', 'setup_done'][step]);
  $('setup-dialog').setAttribute('aria-busy', String(busy));
}
function goTo(next, animate = true) {
  const direction = next < step ? -1 : 1;
  step = next;
  transition?.cancel();
  $('setup-error').hidden = true;
  $('setup-progress').hidden = true;
  document.querySelectorAll('[data-setup-panel]').forEach(panel => { panel.hidden = Number(panel.dataset.setupPanel) !== step; });
  document.querySelectorAll('[data-setup-step]').forEach(item => {
    const index = Number(item.dataset.setupStep);
    item.toggleAttribute('data-complete', index < step);
    if (index === step) item.setAttribute('aria-current', 'step'); else item.removeAttribute('aria-current');
  });
  $('setup-title').textContent = t(['setup_welcome', 'setup_prepare_title', 'setup_test_title', 'setup_complete_title'][step]);
  $('setup-description').textContent = t(['setup_welcome_desc', rerun ? 'setup_prepare_again_desc' : 'setup_prepare_desc', 'setup_test_desc', 'setup_complete_desc'][step]);
  $('setup-test-intro').hidden = Boolean(best);
  $('setup-result').hidden = !best;
  if (step === 3) $('setup-summary').textContent = t(connected ? 'setup_summary_connected' : 'setup_summary_manual');
  controls();
  $('setup-body').scrollTop = 0;
  if (animate && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    transition = $('setup-body').animate([{ opacity: 0, transform: `translateX(${direction * 12}px)` }, { opacity: 1, transform: 'translateX(0)' }], { duration: 240, easing: 'cubic-bezier(0.23, 1, 0.32, 1)' });
  }
  $('setup-title').focus({ preventScroll: true });
}
function setProgress(label, detail = '', value = null) {
  $('setup-progress').hidden = false;
  $('setup-progress-label').textContent = label;
  $('setup-progress-detail').textContent = detail;
  if (value === null) $('setup-progress-bar').removeAttribute('value');
  else $('setup-progress-bar').value = Math.max(0, Math.min(100, value));
}
async function stopForSetup() {
  const pre = await invoke('precheck_tests');
  if (!pre.is_admin) throw new Error(t('wizard_need_admin'));
  if (pre.service_installed || pre.service_running || pre.winws_running) {
    await invoke('stop_zapret');
    const after = await invoke('precheck_tests');
    if (after.service_installed || after.service_running || after.winws_running) throw new Error(t('setup_stop_failed'));
  }
}
async function resetConfiguration() {
  await stopForSetup();
  await invoke('set_game_filter', { mode: 'disabled' });
  await invoke('set_ipset_filter', { mode: 'any' });
  const fakes = await invoke('get_fakes_info');
  for (const [fakeType, fakeName] of [['discord', 'quic_initial_steamcommunity_com'], ['game', 'quic_initial_dbankcloud_ru']]) {
    if (fakes.available_fakes?.some(fake => fake.name === fakeName)) await invoke('set_active_fake', { fakeType, fakeName });
  }
  if (deleteLists) {
    const backup = [];
    for (const filename of USER_LISTS) backup.push({ filename, lines: await invoke('read_user_list', { filename }) });
    try {
      for (const filename of USER_LISTS) await invoke('write_user_list', { filename, lines: [] });
    } catch (error) {
      const recovery = await Promise.allSettled(backup.map(entry => invoke('write_user_list', entry)));
      if (recovery.some(result => result.status === 'rejected')) throw new Error(`${t('setup_list_restore_failed')}\n${error}`);
      throw error;
    }
    deleteLists = false;
  }
  state.pendingRestart = false;
  state.pendingNavId = null;
  state.previousGameFilter = 'all';
  state.previousIPSet = 'any';
  updateRestartBanner();
}
async function prepare() {
  if (prepared) { goTo(2); return; }
  busy = true; controls(); $('setup-error').hidden = true;
  let unlisten;
  try {
    setProgress(t('setup_check_core'));
    if (!await invoke('ensure_binaries_present')) {
      unlisten = await listen('download-progress', event => {
        const percent = Number(event.payload);
        if (Number.isFinite(percent)) setProgress(t(percent < 90 ? 'downloading_core' : 'extracting'), `${percent}%`, percent);
      });
      await invoke('download_and_install_update');
      if (!await invoke('ensure_binaries_present')) throw new Error(t('setup_core_missing'));
    }
    if (rerun) { setProgress(t('setup_resetting')); await resetConfiguration(); }
    const pre = await invoke('precheck_tests');
    if (!pre.strategies_count) throw new Error(t('wizard_no_strategies'));
    await loadStrategies();
    prepared = true; goTo(2);
  } catch (error) { $('setup-progress').hidden = true; errorMessage(error); }
  finally { unlisten?.(); busy = false; controls(); }
}
async function testStrategies() {
  busy = true; cancelled = false; testStarted = false; controls(); $('setup-error').hidden = true;
  const subscriptions = [];
  try {
    setProgress(t('setup_test_preparing'));
    await stopForSetup();
    subscriptions.push(await listen('test-config-start', ({ payload }) => {
      testStarted = true; controls();
      setProgress(t('setup_testing', { index: payload.index, total: payload.total }), payload.name, payload.total ? (payload.index - 1) / payload.total * 100 : 0);
    }));
    const results = await invoke('run_tests', { testType: 'standard', testMode: 'all', _testMode: 'all' });
    if (cancelled) { goTo(2); errorMessage(t('setup_test_cancelled')); return; }
    results.sort((a, b) => (b.score || 0) - (a.score || 0));
    best = results.find(result => result.http_ok > 0) || null;
    const payload = { timestamp: new Date().toISOString(), test_type: 'standard', best: best?.config || null, results };
    await invoke('save_test_results', { payload }); state.cachedTestResults = payload;
    await loadStrategies(); goTo(2);
    if (!best) { errorMessage(t('setup_no_result')); return; }
    $('setup-best-name').textContent = best.config.replace(/\.bat$/i, '');
    $('setup-best-meta').textContent = t('setup_result_meta', { ok: best.http_ok, total: best.http_ok + best.http_error });
  } catch (error) { best = null; goTo(2); errorMessage(error); }
  finally { subscriptions.forEach(unlisten => unlisten()); busy = false; testStarted = false; controls(); }
}
async function applyResult() {
  busy = true; controls(); $('setup-error').hidden = true;
  try {
    const strategy = best.config.replace(/\.bat$/i, '');
    const mode = document.querySelector('input[name="setup-mode"]:checked').value;
    await invoke('start_zapret', { strategy, mode });
    setStrategyValue(strategy, strategy); connected = true; goTo(3);
  } catch (error) { errorMessage(error); }
  finally { busy = false; controls(); }
}
async function closeSetup(completed = false) {
  if (busy) return;
  if (completed) {
    try { localStorage.setItem(COMPLETE_KEY, '1'); localStorage.setItem('zapret.firstrun.dismissed', '1'); } catch { /* Retry next launch if storage is unavailable. */ }
  }
  if (isSetupWindow) {
    try { await invoke('finish_setup_window'); }
    catch (error) { errorMessage(error); }
    return;
  }
  appearanceHome.replaceWith(appearance); $('setup-dialog').close(); state.setupActive = false;
  previousFocus?.focus(); resolveSetup?.(); resolveSetup = null;
  if (rerun) Promise.allSettled([pollStatus(), pollFilters(), pollFakes()]);
}
export async function openSetup({ repeat = false, clearLists = false } = {}) {
  if (state.setupActive) return Promise.resolve();
  if (!isSetupWindow && window.__TAURI__) {
    state.setupActive = true;
    let unlisten;
    try {
      let resolveFinished;
      const finished = new Promise(resolve => { resolveFinished = resolve; });
      unlisten = await listen('setup-finished', async () => {
        try {
          unlisten?.();
          state.setupActive = false;
          syncThemeFromStorage();
          if (repeat) {
            state.pendingRestart = false;
            state.pendingNavId = null;
            updateRestartBanner();
            await loadCachedTestResults();
            await Promise.allSettled([loadStrategies(), pollStatus(), pollFilters(), pollFakes()]);
          }
          await invoke('show_app_window', { force: true });
        } finally { resolveFinished(); }
      });
      await invoke('open_setup_window', { repeat, clearLists });
      await finished;
    } catch (error) {
      unlisten?.();
      state.setupActive = false;
      await invoke('show_app_window', { force: true });
      console.error('Setup window failed:', error);
    }
    return;
  }
  rerun = repeat; deleteLists = clearLists; prepared = false; best = null; connected = false; state.setupActive = true;
  previousFocus = document.activeElement;
  appearance = document.querySelector('.theme-settings');
  appearanceHome = document.createComment('appearance settings'); appearance.before(appearanceHome);
  $('setup-appearance').append(appearance); $('setup-dialog').showModal(); goTo(0, false);
  if (isSetupWindow) {
    await listen('setup-close-requested', () => {
      if (busy) return;
      if (rerun) closeSetup(); else invoke('exit_app');
    });
    await invoke('setup_window_ready');
  }
  return new Promise(resolve => { resolveSetup = resolve; });
}
export function initFirstRun() {
  $('setup-next').addEventListener('click', () => {
    if (busy) return;
    if (step === 0) goTo(1);
    else if (step === 1) prepare();
    else if (step === 2) { if (best) applyResult(); else testStrategies(); }
    else closeSetup(true);
  });
  $('setup-back').addEventListener('click', () => { if (!busy && step > 0) goTo(step - 1); });
  $('setup-skip').addEventListener('click', async () => {
    if (!busy) { goTo(3); return; }
    if (!testStarted || cancelled) return;
    cancelled = true; controls();
    try { await invoke('cancel_tests'); }
    catch (error) { cancelled = false; controls(); errorMessage(error); }
  });
  $('setup-minimize').addEventListener('click', async () => {
    try { await getCurrentWindow().minimize(); }
    catch (error) { console.warn('Could not minimize setup window:', error); }
  });
  $('setup-close').addEventListener('click', () => {
    if (busy) return;
    if (isSetupWindow && !rerun) invoke('exit_app');
    else closeSetup();
  });
  $('setup-dialog').addEventListener('cancel', event => { event.preventDefault(); if (rerun && !busy) closeSetup(); });
  $('setup-restart-btn').addEventListener('click', () => {
    if (state.setupActive || !$('test-wizard-modal').classList.contains('hidden') || state.restartInProgress) return;
    $('setup-delete-lists').checked = false; $('setup-confirm').showModal(); $('setup-confirm-cancel').focus();
  });
  $('setup-confirm-cancel').addEventListener('click', () => $('setup-confirm').close());
  $('setup-confirm-start').addEventListener('click', () => {
    const clearLists = $('setup-delete-lists').checked; $('setup-confirm').close(); openSetup({ repeat: true, clearLists });
  });
}
export async function maybeShowFirstRun() {
  let completed = false;
  try { completed = localStorage.getItem(COMPLETE_KEY) === '1'; } catch { /* Show setup. */ }
  if (completed) {
    try { if (await invoke('ensure_binaries_present')) return; }
    catch (error) { console.warn('Core check failed:', error); }
  }
  await openSetup();
}
