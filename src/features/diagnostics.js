import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';

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
      const labelEl = $('diagnostics-show-all-label');
      if (labelEl) {
        labelEl.textContent = showAll 
          ? t('diagnostics_hide_successful') 
          : t('diagnostics_show_all_hidden', { count: hiddenCount });
      }
      const iconEl = showAllBtn.querySelector('.material-symbols-outlined');
      if (iconEl) {
        iconEl.textContent = showAll ? 'visibility_off' : 'visibility';
      }
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
          <h4 class="font-headline text-sm font-bold text-on-surface">${t('vpn_services_found')}</h4>
          <p class="text-xs text-on-surface-variant mt-1">${result.vpn_services}</p>
          <p class="text-xs text-primary mt-2">${t('vpn_disable_hint')}</p>
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
    runDiagnosticsBtn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('diagnostics_running')}`;
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
            ${t('diagnostics_failed')}: ${err}
          </div>
        `;
      }
    } finally {
      runDiagnosticsBtn.disabled = false;
      runDiagnosticsBtn.innerHTML = `<span class="material-symbols-outlined text-sm">play_arrow</span> ${t('run_diagnostics')}`;
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
    statusEl.textContent = t('clearing_cache');
    statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
    clearDiscordCacheBtn.disabled = true;
    try {
      await invoke('clear_discord_cache');
      statusEl.textContent = t('cache_cleared');
      statusEl.className = 'mt-4 text-sm text-secondary whitespace-pre-line';
    } catch (err) {
      statusEl.textContent = `${t('error')}: ${err}`;
      statusEl.className = 'mt-4 text-sm text-error-dim';
    } finally {
      clearDiscordCacheBtn.disabled = false;
    }
  });

  initSiteChecker();
}

function initSiteChecker() {
  const input = $('site-checker-input');
  const btn = $('run-site-check-btn');
  const resultsDiv = $('site-checker-results');

  if (!btn || !input || !resultsDiv) return;

  btn.addEventListener('click', async () => {
    const domain = input.value.trim();
    if (!domain) return;

    btn.disabled = true;
    input.disabled = true;
    const origText = btn.innerHTML;
    btn.innerHTML = `<span class="material-symbols-outlined text-sm animate-spin">refresh</span> ${t('site_checker_running') || 'Checking...'}`;

    resultsDiv.classList.remove('hidden');
    resultsDiv.innerHTML = `
      <div class="flex items-center justify-center p-4">
        <span class="material-symbols-outlined text-primary text-xl animate-spin mr-2">refresh</span>
        <span class="text-xs text-on-surface-variant">${t('site_checker_running') || 'Checking...'}</span>
      </div>
    `;

    try {
      const res = await invoke('check_site', { domain });
      
      // Run HTTP check in frontend
      let httpStatus = 'blocked';
      let httpMessage = t('site_checker_connection_failed');
      let youtubeVideoBlocked = false;

      if (res.dns_status === 'ok' && res.dns_resolved_ips.length > 0) {
        try {
          const controller = new AbortController();
          const timeoutId = setTimeout(() => controller.abort(), 6000);
          await fetch(`https://${res.domain}`, {
            mode: 'no-cors',
            signal: controller.signal,
            credentials: 'omit',
            cache: 'no-store'
          });
          clearTimeout(timeoutId);
          httpStatus = 'accessible';
          httpMessage = t('site_checker_http_success');
        } catch (err) {
          if (err.name === 'AbortError') {
            httpMessage = t('site_checker_connection_timeout');
          } else {
            httpMessage = t('site_checker_connection_failed');
          }
        }

        // YouTube specific secondary check for googlevideo.com
        const isYouTube = res.domain.includes('youtube') || res.domain === 'youtu.be';
        if (isYouTube && httpStatus === 'accessible') {
          try {
            const controller = new AbortController();
            const timeoutId = setTimeout(() => controller.abort(), 6000);
            await fetch(`https://redirector.googlevideo.com`, {
              mode: 'no-cors',
              signal: controller.signal,
              credentials: 'omit',
              cache: 'no-store'
            });
            clearTimeout(timeoutId);
          } catch (err) {
            youtubeVideoBlocked = true;
          }
        }
      } else {
        httpStatus = 'error';
        httpMessage = t('site_checker_dns_failed');
      }

      res.http_status = httpStatus;
      res.http_message = httpMessage;
      
      // Determine HTTP status icon and color
      let httpIcon = 'error';
      let httpColor = 'text-error-dim';
      let httpBorder = 'border-error-dim/20';
      let recText = t('site_checker_rec_blocked_stopped');

      if (res.http_status === 'accessible') {
        if (youtubeVideoBlocked) {
          httpIcon = 'warning';
          httpColor = 'text-primary';
          httpBorder = 'border-primary/20';
          res.http_message = t('site_checker_youtube_video_blocked_msg');
          recText = t('site_checker_rec_youtube_video_blocked');
        } else {
          httpIcon = 'check_circle';
          httpColor = 'text-secondary';
          httpBorder = 'border-secondary/20';
          recText = t('site_checker_rec_accessible');
        }
      } else {
        if (res.is_zapret_running) {
          recText = t('site_checker_rec_blocked_running');
        } else {
          recText = t('site_checker_rec_blocked_stopped');
        }
      }

      // Determine DNS status icon and color
      const dnsIcon = res.dns_status === 'ok' ? 'check_circle' : 'error';
      const dnsColor = res.dns_status === 'ok' ? 'text-secondary' : 'text-error-dim';
      const dnsBorder = res.dns_status === 'ok' ? 'border-secondary/20' : 'border-error-dim/20';

      // Determine Ping status icon and color
      const pingVal = res.ping_ms !== null ? `${res.ping_ms} ${t('ms')}` : t('timeout');
      const pingIcon = res.ping_ms !== null ? 'check_circle' : 'error';
      const pingColor = res.ping_ms !== null ? 'text-secondary' : 'text-error-dim';
      const pingBorder = res.ping_ms !== null ? 'border-secondary/20' : 'border-error-dim/20';

      // Zapret status badge
      const zapretStatusText = res.is_zapret_running ? t('site_checker_zapret_running') : t('site_checker_zapret_stopped');
      const zapretStatusColor = res.is_zapret_running ? 'text-secondary bg-secondary/10 border-secondary/20' : 'text-error-dim bg-error-dim/10 border-error-dim/20';

      resultsDiv.innerHTML = `
        <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
          <!-- DNS Result -->
          <div class="glass-panel p-4 rounded-xl border ${dnsBorder} flex items-start gap-3">
            <span class="material-symbols-outlined ${dnsColor} text-xl mt-0.5">${dnsIcon}</span>
            <div class="flex-1 min-w-0">
              <h4 class="font-headline text-xs font-bold uppercase tracking-wider text-on-surface-variant">${t('site_checker_dns_title')}</h4>
              <p class="text-sm font-bold text-on-surface mt-1 truncate">${res.dns_status === 'ok' ? t('site_checker_dns_resolved', { count: res.dns_resolved_ips.length }) : t('site_checker_dns_failed')}</p>
              ${res.dns_resolved_ips.length > 0 ? `
                <div class="mt-2 text-[10px] font-mono text-on-surface-variant/80 max-h-16 overflow-y-auto space-y-0.5">
                  ${res.dns_resolved_ips.map(ip => `<div>${ip}</div>`).join('')}
                </div>
              ` : ''}
            </div>
          </div>

          <!-- TCP Ping Result -->
          <div class="glass-panel p-4 rounded-xl border ${pingBorder} flex items-start gap-3">
            <span class="material-symbols-outlined ${pingColor} text-xl mt-0.5">${pingIcon}</span>
            <div>
              <h4 class="font-headline text-xs font-bold uppercase tracking-wider text-on-surface-variant">${t('site_checker_ping_title')}</h4>
              <p class="text-sm font-bold text-on-surface mt-1">${pingVal}</p>
            </div>
          </div>

          <!-- HTTP Result -->
          <div class="glass-panel p-4 rounded-xl border ${httpBorder} flex items-start gap-3">
            <span class="material-symbols-outlined ${httpColor} text-xl mt-0.5">${httpIcon}</span>
            <div>
              <h4 class="font-headline text-xs font-bold uppercase tracking-wider text-on-surface-variant">${t('site_checker_http_title')}</h4>
              <p class="text-sm font-bold text-on-surface mt-1">${res.http_message}</p>
            </div>
          </div>
        </div>

        <!-- Recommendation and Zapret Status -->
        <div class="glass-panel p-5 rounded-xl border border-primary/10 space-y-4">
          <div class="flex items-center justify-between">
            <h4 class="font-headline text-xs font-bold uppercase tracking-widest text-primary/70">${t('site_checker_recommendation')}</h4>
            <span class="px-2.5 py-1 rounded-full border text-[10px] font-bold uppercase tracking-wider ${zapretStatusColor}">
              ${zapretStatusText}
            </span>
          </div>
          <p class="text-xs text-on-surface/90 leading-relaxed">${recText}</p>
        </div>
      `;
    } catch (err) {
      resultsDiv.innerHTML = `
        <div class="bg-white/5 rounded-xl border border-error-dim/30 p-4 text-error-dim text-xs">
          ${t('error')}: ${err}
        </div>
      `;
    } finally {
      btn.disabled = false;
      input.disabled = false;
      btn.innerHTML = origText;
    }
  });
}
