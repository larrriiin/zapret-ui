import { $, invoke } from '../lib/core.js';
import { onLangChange, t } from '../lib/i18n.js';

let lastStatusReport = null;

const STATUS_META = {
  selected: { label: 'system_state_selected', icon: 'check', tone: 'text-primary', badge: 'bg-primary/10 text-primary' },
  running: { label: 'system_state_running', icon: 'check_circle', tone: 'text-secondary', badge: 'bg-secondary/10 text-secondary' },
  stopped: { label: 'system_state_stopped', icon: 'pause_circle', tone: 'text-error-dim', badge: 'bg-error-dim/10 text-error-dim' },
  not_installed: { label: 'system_state_not_installed', icon: 'remove_circle', tone: 'text-on-surface-variant', badge: 'bg-white/5 text-on-surface-variant' },
  unknown: { label: 'system_state_unknown', icon: 'help', tone: 'text-tertiary', badge: 'bg-tertiary/10 text-tertiary' },
};

function makeStatusRow({ icon, labelKey, value, detail }) {
  const meta = STATUS_META[value] || STATUS_META.unknown;
  const row = document.createElement('div');
  row.className = 'flex items-center gap-3 rounded-xl bg-surface-container-high/40 px-4 py-3 min-w-0';

  const itemIcon = document.createElement('span');
  itemIcon.className = 'material-symbols-outlined text-primary/60 text-xl shrink-0';
  itemIcon.textContent = icon;

  const copy = document.createElement('div');
  copy.className = 'min-w-0 flex-1';
  const label = document.createElement('div');
  label.className = 'text-sm font-semibold text-on-surface';
  label.textContent = t(labelKey);
  copy.appendChild(label);
  if (detail) {
    const detailEl = document.createElement('div');
    detailEl.className = 'mt-0.5 text-xs text-on-surface-variant/80 truncate';
    detailEl.title = detail;
    detailEl.textContent = detail;
    copy.appendChild(detailEl);
  }

  const state = document.createElement('span');
  state.className = `shrink-0 inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-bold ${meta.badge}`;
  const stateIcon = document.createElement('span');
  stateIcon.className = `material-symbols-outlined text-[16px] ${meta.tone}`;
  stateIcon.textContent = meta.icon;
  const stateLabel = document.createElement('span');
  stateLabel.textContent = t(meta.label);
  state.append(stateIcon, stateLabel);
  row.append(itemIcon, copy, state);
  return row;
}

export function getSystemSummaryState(report) {
  if (report?.running === false) return 'stopped';
  if (report?.running !== true) return 'attention';
  const ready = report.windivert_service === 'running'
    && report.bypass_process === 'running'
    && Boolean(report.strategy);
  return ready ? 'running' : 'attention';
}

function renderStatusReport(report) {
  const statusContent = $('status-content');
  if (!statusContent || !report) return;
  statusContent.replaceChildren();
  statusContent.setAttribute('aria-busy', 'false');

  const summaryState = getSystemSummaryState(report);
  const healthy = summaryState === 'running';
  const stopped = summaryState === 'stopped';
  const summary = document.createElement('div');
  summary.className = `rounded-xl p-4 ${healthy ? 'bg-secondary/10' : stopped ? 'bg-white/5' : 'bg-tertiary/10'}`;
  const summaryTitle = document.createElement('div');
  summaryTitle.className = `font-headline text-base font-bold ${healthy ? 'text-secondary' : stopped ? 'text-on-surface' : 'text-tertiary'}`;
  summaryTitle.textContent = t(healthy ? 'system_summary_running' : stopped ? 'system_summary_stopped' : 'system_summary_attention');
  const summaryDesc = document.createElement('p');
  summaryDesc.className = 'mt-1 text-sm text-on-surface-variant/80';
  summaryDesc.textContent = t(healthy ? 'system_summary_running_desc' : stopped ? 'system_summary_stopped_desc' : 'system_summary_attention_desc');
  summary.append(summaryTitle, summaryDesc);
  statusContent.appendChild(summary);

  statusContent.append(
    makeStatusRow({ icon: 'route', labelKey: 'system_status_strategy', value: report.strategy ? 'selected' : 'unknown', detail: report.strategy || t('system_strategy_unknown') }),
    makeStatusRow({ icon: 'settings_ethernet', labelKey: 'system_status_zapret', value: report.zapret_service }),
    makeStatusRow({ icon: 'security', labelKey: 'system_status_windivert', value: report.windivert_service }),
    makeStatusRow({ icon: 'memory', labelKey: 'system_status_bypass', value: report.bypass_process, detail: 'winws.exe' }),
  );
}

function renderStatusMessage(message, tone = 'neutral') {
  const statusContent = $('status-content');
  if (!statusContent) return;
  statusContent.replaceChildren();
  statusContent.setAttribute('aria-busy', tone === 'loading' ? 'true' : 'false');
  const messageEl = document.createElement('div');
  messageEl.className = `rounded-xl p-5 text-sm ${tone === 'error' ? 'bg-error-dim/10 text-error-dim' : 'bg-surface-container-high/70 text-on-surface-variant'}`;
  messageEl.textContent = message;
  statusContent.appendChild(messageEl);
}

export function initStatusCheck() {
  const checkStatusBtn = $('check-status-btn');
  const statusModal = $('status-modal');
  const statusContent = $('status-content');
  const statusModalClose = $('status-modal-close');
  const statusModalDone = $('status-modal-done');

  if (checkStatusBtn && statusModal && statusContent) {
    checkStatusBtn.addEventListener('click', async () => {
      renderStatusMessage(t('status_checking_realtime'), 'loading');
      statusModal.classList.remove('hidden');
      try {
        lastStatusReport = await invoke('check_status_full');
        renderStatusReport(lastStatusReport);
      } catch (err) {
        renderStatusMessage(`${t('status_check_failed')}: ${err}`, 'error');
      }
    });
  }

  const closeModal = () => statusModal?.classList.add('hidden');
  statusModalClose?.addEventListener('click', closeModal);
  statusModalDone?.addEventListener('click', closeModal);
  statusModal?.addEventListener('click', (event) => {
    if (event.target === statusModal) closeModal();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !statusModal?.classList.contains('hidden')) closeModal();
  });
  onLangChange(() => {
    if (lastStatusReport && !statusModal?.classList.contains('hidden')) renderStatusReport(lastStatusReport);
  });
}
