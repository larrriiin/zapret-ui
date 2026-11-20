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

export function makeStatusRow({ icon, labelKey, value, detail, valueLabel, showBadge = true }) {
  const row = document.createElement('div');
  row.className = 'flex items-center gap-3 rounded-xl bg-surface-container-high/40 px-4 py-3 min-w-0';

  const itemIcon = document.createElement('span');
  itemIcon.className = `${icon === 'telegram' ? 'telegram-icon' : 'material-symbols-outlined'} text-primary/60 text-xl shrink-0`;
  itemIcon.textContent = icon === 'telegram' ? '' : icon;
  itemIcon.setAttribute('aria-hidden', 'true');

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
  row.append(itemIcon, copy);
  if (!showBadge) return row;

  const meta = STATUS_META[value] || STATUS_META.unknown;
  const state = document.createElement('span');
  state.className = `shrink-0 inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-xs font-bold ${meta.badge}`;
  const stateIcon = document.createElement('span');
  stateIcon.className = `material-symbols-outlined text-[16px] ${meta.tone}`;
  stateIcon.textContent = meta.icon;
  const stateLabel = document.createElement('span');
  stateLabel.textContent = valueLabel || t(meta.label);
  state.append(stateIcon, stateLabel);
  row.append(state);
  return row;
}

export function makeStatusSummary({ state, title, detail }) {
  const healthy = state === 'running';
  const stopped = state === 'stopped';
  const summary = document.createElement('div');
  summary.className = `rounded-xl p-4 ${healthy ? 'bg-secondary/10' : stopped ? 'bg-white/5' : 'bg-tertiary/10'}`;
  const heading = document.createElement('div');
  heading.className = `font-headline text-base font-bold ${healthy ? 'text-secondary' : stopped ? 'text-on-surface' : 'text-tertiary'}`;
  heading.textContent = title;
  summary.append(heading);
  if (detail) {
    const description = document.createElement('p');
    description.className = 'mt-1 text-sm text-on-surface-variant/80';
    description.textContent = detail;
    summary.append(description);
  }
  return summary;
}

export function makeStatusMessage(message, tone = 'neutral') {
  const messageEl = document.createElement('div');
  messageEl.className = `rounded-xl p-5 text-sm ${tone === 'error' ? 'bg-error-dim/10 text-error-dim' : 'bg-surface-container-high/70 text-on-surface-variant'}`;
  messageEl.textContent = message;
  return messageEl;
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

  const state = getSystemSummaryState(report);
  statusContent.append(
    makeStatusSummary({ state, title: t(`system_summary_${state}`), detail: t(`system_summary_${state}_desc`) }),
    makeStatusRow({ icon: 'route', labelKey: 'system_status_strategy', value: report.strategy ? 'selected' : 'unknown', detail: report.strategy || t('system_strategy_unknown') }),
    makeStatusRow({ icon: 'settings_ethernet', labelKey: 'system_status_zapret', value: report.zapret_service }),
    makeStatusRow({ icon: 'security', labelKey: 'system_status_windivert', value: report.windivert_service }),
    makeStatusRow({ icon: 'memory', labelKey: 'system_status_bypass', value: report.bypass_process, detail: 'winws.exe' }),
  );
}

function renderStatusMessage(message, tone = 'neutral') {
  const statusContent = $('status-content');
  if (!statusContent) return;
  statusContent.setAttribute('aria-busy', tone === 'loading' ? 'true' : 'false');
  statusContent.replaceChildren(makeStatusMessage(message, tone));
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
