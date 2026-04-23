import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';
import { escapeHtml } from '../lib/dom.js';

let lastDiagnosticsResults = null;
let showingAllDiagnostics = false;

function buildDiagnosticsReport(result) {
  if (!result || !result.checks) return '';
  const lines = [];
  lines.push(`Zapret UI diagnostics — ${new Date().toISOString()}`);
  lines.push('');
  for (const check of result.checks) {
    const status = (check.status || '').toUpperCase();
    lines.push(`[${status}] ${check.name}`);
    if (check.message) lines.push(`    ${check.message}`);
    if (check.link) lines.push(`    ${check.link}`);
  }
  if (result.vpn_services) {
    lines.push('');
    lines.push('[INFO] VPN services found');
    lines.push(`    ${result.vpn_services}`);
  }
  return lines.join('\n');
}

async function copyDiagnosticsReport() {
  const copyReportLabel = $('diagnostics-copy-label');
  if (!lastDiagnosticsResults) return;
  const report = buildDiagnosticsReport(lastDiagnosticsResults);
  try {
    await navigator.clipboard.writeText(report);
  } catch {
    const ta = document.createElement('textarea');
    ta.value = report;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand('copy'); } catch {}
    document.body.removeChild(ta);
  }
  if (copyReportLabel) {
    const original = t('copy_report');
    copyReportLabel.textContent = t('report_copied');
    setTimeout(() => { copyReportLabel.textContent = original; }, 1500);
  }
}

function renderDiagnostics(result, showAll) {
  const diagnosticsResults = $('diagnostics-results');
  const showAllBtn = $('diagnostics-show-all-btn');
  if (!diagnosticsResults) return;
  diagnosticsResults.innerHTML = '';
  if (!result || !result.checks) return;

  let hiddenCount = 0;

  result.checks.forEach((check) => {
    const isSuccess = check.status === 'passed';
    if (!showAll && isSuccess) {
      hiddenCount++;
      return;
    }

    const row = document.createElement('div');
    row.className = 'bg-white/5 rounded-xl border p-4 flex items-start gap-3 transition-opacity duration-300';

    let icon, iconColor, borderColor;
    if (isSuccess) {
      icon = 'check_circle';
      iconColor = 'text-secondary';
      borderColor = 'border-secondary/30';
    } else if (check.status === 'warning') {
      icon = 'warning';
      iconColor = 'text-primary';
      borderColor = 'border-primary/30';
    } else {
      icon = 'error';
      iconColor = 'text-error-dim';
      borderColor = 'border-error-dim/30';
    }
    row.classList.add(borderColor);

    let linkHtml = '';
    if (check.link) {
      linkHtml = `<a href="${check.link}" target="_blank" class="text-xs text-primary hover:underline mt-1 block">${check.link}</a>`;
    }

    row.innerHTML = `
      <span class="material-symbols-outlined ${iconColor} text-xl mt-0.5">${icon}</span>
      <div class="flex-1">
        <h4 class="font-headline text-sm font-bold text-on-surface">${check.name}</h4>
        <p class="text-xs text-on-surface-variant mt-1">${check.message}</p>
        ${linkHtml}
      </div>
    `;
    diagnosticsResults.appendChild(row);
  });

  if (showAllBtn) {
    if (hiddenCount > 0 || showAll) {
      showAllBtn.classList.remove('hidden');
      showAllBtn.textContent = showAll ? 'Hide Successful Checks' : `Show All Checks (${hiddenCount} hidden)`;
    } else {
      showAllBtn.classList.add('hidden');
    }
  }

  if (result.vpn_services) {
    const vpnRow = document.createElement('div');
    vpnRow.className = 'bg-white/5 rounded-xl border border-primary/30 p-4 mt-3';
    vpnRow.innerHTML = `
      <div class="flex items-start gap-3">
        <span class="material-symbols-outlined text-primary text-xl mt-0.5">vpn_key</span>
        <div class="flex-1">
          <h4 class="font-headline text-sm font-bold text-on-surface">VPN Services Found</h4>
          <p class="text-xs text-on-surface-variant mt-1">${result.vpn_services}</p>
          <p class="text-xs text-primary mt-2">Make sure that all VPNs are disabled</p>
        </div>
      </div>
    `;
    diagnosticsResults.appendChild(vpnRow);
  }
}

export function initDiagnostics() {
  const runDiagnosticsBtn = $('run-diagnostics-btn');
  const diagnosticsResults = $('diagnostics-results');
  const discordCacheSection = $('discord-cache-section');
  const showAllBtn = $('diagnostics-show-all-btn');
  const copyReportBtn = $('diagnostics-copy-btn');

  copyReportBtn?.addEventListener('click', copyDiagnosticsReport);

  runDiagnosticsBtn?.addEventListener('click', async () => {
    runDiagnosticsBtn.disabled = true;
    runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm animate-spin">refresh</span> Running...';
    if (diagnosticsResults) {
      diagnosticsResults.innerHTML = '';
      diagnosticsResults.classList.remove('hidden');
    }
    discordCacheSection?.classList.add('hidden');
    showAllBtn?.classList.add('hidden');
    copyReportBtn?.classList.add('hidden');
    showingAllDiagnostics = false;

    try {
      const result = await invoke('run_diagnostics');
      lastDiagnosticsResults = result;
      renderDiagnostics(result, false);
      discordCacheSection?.classList.remove('hidden');
      if (copyReportBtn && result && result.checks && result.checks.length) {
        copyReportBtn.classList.remove('hidden');
      }
    } catch (err) {
      if (diagnosticsResults) {
        diagnosticsResults.innerHTML = `
          <div class="bg-white/5 rounded-xl border border-error-dim/30 p-4 text-error-dim text-sm">
            Failed to run diagnostics: ${err}
          </div>
        `;
      }
    } finally {
      runDiagnosticsBtn.disabled = false;
      runDiagnosticsBtn.innerHTML = '<span class="material-symbols-outlined text-sm">play_arrow</span> Run Diagnostics';
    }
  });

  showAllBtn?.addEventListener('click', () => {
    showingAllDiagnostics = !showingAllDiagnostics;
    renderDiagnostics(lastDiagnosticsResults, showingAllDiagnostics);
  });

  const clearDiscordCacheBtn = $('clear-discord-cache-btn');
  clearDiscordCacheBtn?.addEventListener('click', async () => {
    const statusEl = $('discord-cache-status');
    if (!statusEl) return;
    statusEl.classList.remove('hidden');
    statusEl.innerHTML = 'Clearing...';
    statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
    clearDiscordCacheBtn.disabled = true;
    try {
      const result = await invoke('clear_discord_cache');
      statusEl.innerHTML = escapeHtml(result).replace(/\n/g, '<br>');
      statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
    } catch (err) {
      statusEl.textContent = 'Error: ' + err;
      statusEl.className = 'mt-4 text-sm text-error-dim';
    } finally {
      clearDiscordCacheBtn.disabled = false;
    }
  });
}
