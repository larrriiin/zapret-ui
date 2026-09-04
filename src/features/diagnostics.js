import { $, invoke } from '../lib/core.js';
import { getCurrentLang, onLangChange, t } from '../lib/i18n.js';

let lastDiagnosticsResults = null;
let showingAllDiagnostics = false;

export const DIAGNOSTIC_NAME_KEYS = {
  'Base Filtering Engine': 'diagnostic_bfe_name',
  'System Proxy': 'diagnostic_proxy_name',
  'TCP Timestamps': 'diagnostic_tcp_timestamps_name',
  Adguard: 'diagnostic_adguard_name',
  'Killer Network Service': 'diagnostic_killer_name',
  'Intel Connectivity Network Service': 'diagnostic_intel_connectivity_name',
  'Check Point': 'diagnostic_checkpoint_name',
  SmartByte: 'diagnostic_smartbyte_name',
  'VPN Services': 'diagnostic_vpn_name',
  'Secure DNS': 'diagnostic_secure_dns_name',
  'Hosts File': 'diagnostic_hosts_name',
  WinDivert: 'diagnostic_windivert_name',
};

export const DIAGNOSTIC_MESSAGE_KEYS = {
  'Service is running': 'diagnostic_bfe_running',
  'Service is not running. This service is required for zapret to work': 'diagnostic_bfe_stopped',
  'Failed to check service status': 'diagnostic_bfe_check_failed',
  'No system proxy detected': 'diagnostic_proxy_not_detected',
  'Proxy check passed': 'diagnostic_proxy_check_passed',
  'TCP timestamps are enabled': 'diagnostic_tcp_timestamps_enabled',
  'TCP timestamps were disabled. Attempted to enable them.': 'diagnostic_tcp_timestamps_enabled_automatically',
  'Failed to check TCP timestamps': 'diagnostic_tcp_timestamps_check_failed',
  'Adguard process found. Adguard may cause problems with Discord': 'diagnostic_adguard_found',
  'Adguard not detected': 'diagnostic_adguard_not_detected',
  'Adguard check passed': 'diagnostic_adguard_check_passed',
  'Killer services found. Killer conflicts with zapret': 'diagnostic_killer_found',
  'Killer services not detected': 'diagnostic_killer_not_detected',
  'Killer check passed': 'diagnostic_killer_check_passed',
  'Intel Connectivity Network Service found. It conflicts with zapret': 'diagnostic_intel_connectivity_found',
  'Intel Connectivity service not detected': 'diagnostic_intel_connectivity_not_detected',
  'Intel Connectivity check passed': 'diagnostic_intel_connectivity_check_passed',
  'Check Point services found. Check Point conflicts with zapret': 'diagnostic_checkpoint_found',
  'Check Point services not detected': 'diagnostic_checkpoint_not_detected',
  'Check Point check passed': 'diagnostic_checkpoint_check_passed',
  'SmartByte services found. SmartByte conflicts with zapret': 'diagnostic_smartbyte_found',
  'SmartByte services not detected': 'diagnostic_smartbyte_not_detected',
  'SmartByte check passed': 'diagnostic_smartbyte_check_passed',
  'VPN services found. Some VPNs can conflict with zapret': 'diagnostic_vpn_found',
  'No VPN services detected': 'diagnostic_vpn_not_detected',
  'VPN check passed': 'diagnostic_vpn_check_passed',
  'Make sure you have configured secure DNS in a browser with some non-default DNS service provider. If you use Windows 11 you can configure encrypted DNS in the Settings to hide this warning': 'diagnostic_secure_dns_not_configured',
  'Secure DNS is configured': 'diagnostic_secure_dns_configured',
  'Failed to check DNS configuration': 'diagnostic_secure_dns_check_failed',
  'Your hosts file contains entries for youtube.com or youtu.be. This may cause problems with YouTube access': 'diagnostic_hosts_youtube_found',
  'No YouTube entries in hosts file': 'diagnostic_hosts_clean',
  'WinDivert driver is running': 'diagnostic_windivert_running',
  'WinDivert driver not active (will be started when needed)': 'diagnostic_windivert_inactive',
  'WinDivert check passed': 'diagnostic_windivert_check_passed',
};

function getDiagnosticName(check) {
  const key = DIAGNOSTIC_NAME_KEYS[check?.name];
  return key ? t(key) : (check?.name || t('diagnostic_unknown_name'));
}

function getDiagnosticMessage(check) {
  const message = check?.message || '';
  const proxyPrefix = 'System proxy is enabled: ';
  const proxySuffix = ". Make sure it's valid or disable it if you don't use a proxy";
  if (message.startsWith(proxyPrefix) && message.endsWith(proxySuffix)) {
    const proxy = message.slice(proxyPrefix.length, -proxySuffix.length);
    return t('diagnostic_proxy_enabled', { proxy });
  }
  const key = DIAGNOSTIC_MESSAGE_KEYS[message];
  return key ? t(key) : message;
}

function getDiagnosticStatus(status) {
  const key = `diagnostics_status_${status}`;
  const translated = t(key);
  return translated === key ? status : translated;
}

function buildDiagnosticsReport(result) {
  if (!result || !result.checks) return '';
  const lines = [];
  const locale = getCurrentLang() === 'ru' ? 'ru-RU' : 'en-US';
  const timestamp = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'medium' }).format(new Date());
  lines.push(`${t('diagnostics_report_title')} — ${timestamp}`);
  lines.push('');
  for (const check of result.checks) {
    const status = getDiagnosticStatus(check.status).toUpperCase();
    lines.push(`[${status}] ${getDiagnosticName(check)}`);
    if (check.message) lines.push(`    ${getDiagnosticMessage(check)}`);
    if (check.link) lines.push(`    ${check.link}`);
  }
  if (result.vpn_services) {
    lines.push('');
    lines.push(`[${t('diagnostics_status_info').toUpperCase()}] ${t('vpn_services_found')}`);
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

    const statusIcon = document.createElement('span');
    statusIcon.className = `material-symbols-outlined ${iconColor} text-xl mt-0.5 shrink-0`;
    statusIcon.textContent = icon;

    const content = document.createElement('div');
    content.className = 'flex-1 min-w-0';
    const title = document.createElement('h4');
    title.className = 'font-headline text-sm font-bold text-on-surface';
    title.textContent = getDiagnosticName(check);
    const message = document.createElement('p');
    message.className = 'text-xs text-on-surface-variant mt-1 leading-relaxed break-words';
    message.textContent = getDiagnosticMessage(check);
    content.append(title, message);

    if (check.link) {
      const link = document.createElement('a');
      link.href = check.link;
      link.target = '_blank';
      link.rel = 'noopener noreferrer';
      link.className = 'text-xs text-primary hover:underline underline-offset-4 mt-1 block break-all';
      link.textContent = check.link;
      content.appendChild(link);
    }

    row.append(statusIcon, content);
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
    const wrapper = document.createElement('div');
    wrapper.className = 'flex items-start gap-3';
    const icon = document.createElement('span');
    icon.className = 'material-symbols-outlined text-primary text-xl mt-0.5 shrink-0';
    icon.textContent = 'vpn_key';
    const content = document.createElement('div');
    content.className = 'flex-1 min-w-0';
    const title = document.createElement('h4');
    title.className = 'font-headline text-sm font-bold text-on-surface';
    title.textContent = t('vpn_services_found');
    const services = document.createElement('p');
    services.className = 'text-xs text-on-surface-variant mt-1 leading-relaxed break-words';
    services.textContent = result.vpn_services;
    const hint = document.createElement('p');
    hint.className = 'text-xs text-primary mt-2';
    hint.textContent = t('vpn_disable_hint');
    content.append(title, services, hint);
    wrapper.append(icon, content);
    vpnRow.appendChild(wrapper);
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
        diagnosticsResults.replaceChildren();
        const error = document.createElement('div');
        error.className = 'bg-white/5 rounded-xl border border-error-dim/30 p-4 text-error-dim text-sm break-words';
        error.textContent = `${t('diagnostics_failed')}: ${err}`;
        diagnosticsResults.appendChild(error);
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
  onLangChange(() => {
    if (lastDiagnosticsResults) renderDiagnostics(lastDiagnosticsResults, showingAllDiagnostics);
  });
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
