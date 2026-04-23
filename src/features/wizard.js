import { $, invoke, listen } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { escapeHtml } from '../lib/dom.js';
import { state } from '../lib/state.js';
import { setStrategyValue, loadStrategies } from './strategies.js';
import { pollStatus } from './status.js';

const WizardState = { Hidden: 'hidden', Preflight: 'preflight', Progress: 'progress', Results: 'results' };
const logColors = {
  error: 'text-error-dim',
  warning: 'text-primary',
  success: 'text-secondary',
  separator: 'text-on-surface-variant',
  config: 'text-secondary font-bold',
  info: 'text-on-surface/80',
};

let testsRunning = false;
let selectedTestType = 'standard';
let wizardLastTestType = 'standard';
let wizardUnlisten = { progress: null, configStart: null, best: null };

function showWizardStep(step) {
  const modal = $('test-wizard-modal');
  if (!modal) return;
  if (step === WizardState.Hidden) {
    modal.classList.add('hidden');
    return;
  }
  modal.classList.remove('hidden');
  ['wizard-preflight', 'wizard-progress', 'wizard-results'].forEach((id) => {
    const el = $(id);
    if (el) el.classList.toggle('hidden', !id.endsWith(step));
  });
}

async function unlistenWizard() {
  for (const key of Object.keys(wizardUnlisten)) {
    const fn = wizardUnlisten[key];
    if (typeof fn === 'function') { try { fn(); } catch {} }
    wizardUnlisten[key] = null;
  }
}

async function startWizardProgress(testType) {
  wizardLastTestType = testType;
  testsRunning = true;
  const runTestsBtn = $('run-tests-btn');
  if (runTestsBtn) runTestsBtn.disabled = true;
  showWizardStep(WizardState.Progress);
  $('wizard-overall-bar').style.width = '0%';
  $('wizard-overall-counter').textContent = '0 / 0';
  $('wizard-current-name').textContent = '—';
  $('wizard-best-so-far').classList.add('hidden');
  const logEl = $('wizard-log');
  logEl.innerHTML = '';

  wizardUnlisten.progress = await listen('test-progress', (event) => {
    const { line, kind } = event.payload;
    const row = document.createElement('div');
    row.className = logColors[kind] || 'text-on-surface/80';
    row.textContent = line;
    logEl.appendChild(row);
    logEl.scrollTop = logEl.scrollHeight;
  });
  wizardUnlisten.configStart = await listen('test-config-start', (event) => {
    const { index, total, name } = event.payload;
    $('wizard-overall-counter').textContent = `${index} / ${total}`;
    const pct = total > 0 ? Math.min(100, ((index - 1) / total) * 100) : 0;
    $('wizard-overall-bar').style.width = `${pct}%`;
    $('wizard-current-name').textContent = name;
  });
  wizardUnlisten.best = await listen('test-best', (event) => {
    const { config } = event.payload;
    $('wizard-best-so-far').classList.remove('hidden');
    $('wizard-best-name').textContent = config;
  });

  const hero = $('hero-status');
  if (hero) {
    hero.textContent = t('testing');
    hero.className = 'text-primary';
  }
  const header = $('header-status');
  if (header) header.innerHTML = `<span class="text-primary">${t('status_label')}:</span> <span class="text-primary">${t('testing')}</span>`;

  try {
    const results = await invoke('run_tests', { testType, testMode: 'all' });
    $('wizard-overall-bar').style.width = '100%';
    results.sort((a, b) => (b.score || 0) - (a.score || 0));
    const best = results[0] || null;
    const payload = {
      timestamp: new Date().toISOString(),
      test_type: testType,
      best: best ? best.config : null,
      results,
    };
    try { await invoke('save_test_results', { payload }); } catch (err) { console.warn('save_test_results failed:', err); }
    state.cachedTestResults = payload;
    renderWizardResults(results, best);
    await loadStrategies();
  } catch (err) {
    const logBox = $('wizard-log');
    if (logBox) {
      const row = document.createElement('div');
      row.className = 'text-error-dim';
      row.textContent = `${t('error')}: ${err}`;
      logBox.appendChild(row);
      logBox.scrollTop = logBox.scrollHeight;
    }
  } finally {
    await unlistenWizard();
    testsRunning = false;
    if (runTestsBtn) runTestsBtn.disabled = false;
    await pollStatus();
  }
}

function renderWizardResults(results, best) {
  showWizardStep(WizardState.Results);
  const box = $('wizard-best-box');
  if (best) {
    box.classList.remove('hidden');
    $('wizard-best-final-name').textContent = best.config.replace(/\.bat$/i, '');
    const pingTxt = best.avg_ping_ms > 0 ? `${best.avg_ping_ms} ${t('ms')}` : '—';
    const total = best.http_ok + best.http_error;
    $('wizard-best-final-meta').textContent = `HTTP: ${best.http_ok}/${total} · ${t('ping_label')}: ${pingTxt}`;
    const applyBest = async (mode) => {
      try {
        const strategyName = best.config.replace(/\.bat$/i, '');
        setStrategyValue(strategyName, strategyName);
        await invoke('start_zapret', { strategy: strategyName, mode });
      } catch (err) {
        console.error('Apply best failed:', err);
      }
      showWizardStep(WizardState.Hidden);
      await pollStatus();
    };
    $('wizard-apply-service-btn').onclick = () => applyBest('service');
    $('wizard-apply-temp-btn').onclick = () => applyBest('temporary');
  } else {
    box.classList.add('hidden');
  }

  const list = $('wizard-results-list');
  list.innerHTML = '';
  results.forEach((r) => {
    const row = document.createElement('div');
    const borderColor = r.status === 'success' ? 'border-secondary/30' : r.status === 'partial' ? 'border-primary/30' : 'border-error-dim/30';
    const icon = r.status === 'success' ? 'check_circle' : r.status === 'partial' ? 'warning' : 'error';
    const iconColor = r.status === 'success' ? 'text-secondary' : r.status === 'partial' ? 'text-primary' : 'text-error-dim';
    const isBest = best && r.config === best.config;
    const pingTxt = r.avg_ping_ms > 0 ? `${r.avg_ping_ms} ${t('ms')}` : '—';
    const total = r.http_ok + r.http_error;
    row.className = `rounded-xl border ${borderColor} p-3 flex items-center justify-between gap-3`;
    row.innerHTML = `
      <div class="flex items-center gap-2 min-w-0 flex-1">
        <span class="material-symbols-outlined ${iconColor} text-base shrink-0">${icon}</span>
        <span class="font-mono text-xs text-on-surface truncate">${escapeHtml(r.config.replace(/\.bat$/i, ''))}</span>
        ${isBest ? `<span class="text-[9px] bg-secondary/20 text-secondary px-2 py-0.5 rounded-full uppercase tracking-widest shrink-0">${t('wizard_best_badge')}</span>` : ''}
      </div>
      <div class="text-[10px] text-on-surface-variant text-right shrink-0">
        HTTP ${r.http_ok}/${total} · Ping ${pingTxt}
      </div>
    `;
    list.appendChild(row);
  });
}

function renderPreflight(blockers, testType) {
  const box = $('wizard-preflight-blockers');
  box.innerHTML = '';
  const hasOnlyFixable = blockers.length > 0 && blockers.every((b) => b.action === 'stop');
  blockers.forEach((b) => {
    const row = document.createElement('div');
    const borderColor = b.severity === 'error' ? 'border-error-dim/40' : 'border-primary/40';
    row.className = `rounded-xl border ${borderColor} p-3 flex items-center gap-3`;
    row.innerHTML = `
      <span class="material-symbols-outlined text-primary text-xl">${b.icon}</span>
      <span class="text-sm text-on-surface flex-1">${escapeHtml(b.text)}</span>
    `;
    box.appendChild(row);
  });

  const fixBtn = $('wizard-preflight-fix-btn');
  fixBtn.disabled = !hasOnlyFixable;
  fixBtn.classList.toggle('opacity-50', !hasOnlyFixable);
  fixBtn.classList.toggle('cursor-not-allowed', !hasOnlyFixable);
  fixBtn.onclick = async () => {
    fixBtn.disabled = true;
    try {
      await invoke('stop_zapret');
      await new Promise((r) => setTimeout(r, 1500));
      await runWizard(testType);
    } catch (err) {
      console.error('stop_zapret failed:', err);
      fixBtn.disabled = false;
    }
  };
}

export async function runWizard(testType) {
  showWizardStep(WizardState.Preflight);
  let pre;
  try {
    pre = await invoke('precheck_tests');
  } catch (err) {
    console.error('precheck_tests failed:', err);
    showWizardStep(WizardState.Hidden);
    return;
  }

  const blockers = [];
  if (!pre.is_admin) blockers.push({ severity: 'error', icon: 'shield', text: t('wizard_need_admin') });
  if (pre.service_installed || pre.service_running) {
    blockers.push({ severity: 'warn', icon: 'build', text: t('wizard_service_blocker'), action: 'stop' });
  } else if (pre.winws_running) {
    blockers.push({ severity: 'warn', icon: 'build', text: t('wizard_winws_blocker'), action: 'stop' });
  }
  if (pre.strategies_count === 0) {
    blockers.push({ severity: 'error', icon: 'folder_off', text: t('wizard_no_strategies') });
  }

  if (blockers.length === 0) {
    await startWizardProgress(testType);
    return;
  }
  renderPreflight(blockers, testType);
}

export function getSelectedTestType() {
  return selectedTestType;
}

export function initWizard() {
  const legacyNodes = ['tests-status', 'tests-log', 'tests-results'];
  legacyNodes.forEach((id) => { $(id)?.classList.add('hidden'); });

  document.querySelectorAll('[data-type]').forEach((btn) => {
    btn.addEventListener('click', () => {
      if (testsRunning) return;
      selectedTestType = btn.dataset.type;
      document.querySelectorAll('[data-type]').forEach((b) => {
        b.classList.remove('card-active', 'text-on-background');
        b.classList.add('text-on-surface-variant', 'border-outline-variant/30');
        b.classList.remove('border-primary/30');
      });
      btn.classList.add('card-active', 'border-primary/30');
      btn.classList.remove('text-on-surface-variant', 'border-outline-variant/30');
    });
  });

  $('run-tests-btn')?.addEventListener('click', async () => {
    if (testsRunning) return;
    await runWizard(selectedTestType);
  });

  $('wizard-close-btn')?.addEventListener('click', () => {
    if (testsRunning) invoke('cancel_tests').catch(() => {});
    else showWizardStep(WizardState.Hidden);
  });
  $('wizard-preflight-cancel-btn')?.addEventListener('click', () => showWizardStep(WizardState.Hidden));
  $('wizard-cancel-tests-btn')?.addEventListener('click', async () => {
    try { await invoke('cancel_tests'); } catch (err) { console.error('cancel_tests:', err); }
  });
  $('wizard-done-btn')?.addEventListener('click', () => showWizardStep(WizardState.Hidden));
  $('wizard-rerun-btn')?.addEventListener('click', async () => { await runWizard(wizardLastTestType); });

  const cancelTestsBtn = $('cancel-tests-btn');
  cancelTestsBtn?.addEventListener('click', async () => {
    cancelTestsBtn.disabled = true;
    try { await invoke('cancel_tests'); } catch (err) { console.error('Cancel error:', err); }
  });
}
