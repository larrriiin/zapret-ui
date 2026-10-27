import '/styles.css';
import { mountComponents } from '/components/index.js';
import { initI18n, setLanguage, onLangChange } from '/lib/i18n.js';
import { initWarp } from '/features/warp.js';
import { updateStatusUI } from '/features/status.js';
import { initTheme } from '/features/theme.js';
import { initStrategyDropdown, loadStrategies } from '/features/strategies.js';
import { initInfoModals } from '/features/info-modals.js';
import { initStatusCheck } from '/features/status-check.js';
import { initNavigation, showSection } from '/features/navigation.js';
import { initTrafficMonitor } from '/features/traffic.js';

mountComponents();
localStorage.setItem('zapret_theme', 'graphite');
localStorage.setItem('zapret_accents', JSON.stringify({graphite:'lavender'}));
initTheme(); initI18n();
const $ = id => document.getElementById(id);
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
let installed = false, connected = false, mode = 'warp', port = 40000, failed = false;
let inFlight = 0, maximum = 0, calls = [];
let startupBusy = 2;
let pendingConnection = false;
let protocol = 'MASQUE', protocolEditable = true, probeFailure = false;
let automation = { restore_rules: false };
let sites = { domains: ['example.org'], enabled: false, runtime_active: false };
let activationFailure = false;
let applications = { paths: [], enabled: false, active_connections: 0, total_connections: 0, failed_connections: 0, error: null };
const modes = ['doh','dot','warp','warp+dot','warp+doh','proxy','tunnel_only'];
window.__TAURI__ = { core: { invoke: async (command, args) => {
  if (command === 'notify_available_updates') return false;
  if (command === 'get_traffic_snapshot') return { running: false, connections: [], events: [] };
  if (command === 'get_strategies') return ['general (ALT12)', 'general'];
  if (command === 'check_status_full') return {running:true, strategy:'general (ALT12)', windivert_service:'running', bypass_process:'running', zapret_service:'running'};
  if (!command.includes('warp')) return {};
  calls.push(command); maximum = Math.max(maximum, ++inFlight);
  await wait(80); inFlight--;
  if (command === 'get_warp_status' && startupBusy > 0) { startupBusy--; throw {code:'warp_busy',detail:''}; }
  if (failed) throw {code:'warp_cli_error', detail:'Test failure'};
  if (command === 'choose_warp_applications') return ['C:\\Games\\League of Legends\\LeagueClientUx.exe'];
  if (command === 'connect_warp') connected = !pendingConnection;
  if (command === 'disconnect_warp') { connected = false; pendingConnection = false; }
  if (command === 'set_warp_mode') mode = args.mode;
  if (command === 'set_warp_proxy_port') port = args.port;
  if (command === 'save_warp_sites' || command === 'set_warp_sites') {
    if (args.domains.some(domain => domain.includes('/'))) throw {code:'warp_sites_invalid', detail:''};
    sites.domains = [...new Set(args.domains.map(domain => domain.toLowerCase()))];
    if (command === 'set_warp_sites') sites.enabled = args.enabled;
  }
  if (command === 'save_warp_applications' || command === 'set_warp_applications') {
    applications.paths = args.paths;
    if (command === 'set_warp_applications') applications.enabled = args.enabled;
  }
  if (['set_warp_sites','set_warp_applications'].includes(command) && args.enabled) {
    if (activationFailure) { sites.runtime_active = false; applications.runtime_active = false; connected = false; throw {code:'warp_proxy_unavailable',detail:'Mock connection refused'}; }
    if (!installed) throw {code:'warp_not_installed',detail:''};
    if (['warp','warp+dot','warp+doh','tunnel_only'].includes(mode) && !args.confirmed) throw {code:'warp_confirm_proxy',detail:''};
    mode = 'proxy'; protocol = 'MASQUE'; connected = true;
  }
  if (command === 'set_warp_automation') automation = args.preferences;
  if (command === 'set_warp_protocol') {
    if (args.protocol === 'WireGuard' && mode === 'proxy') {
      if (!args.confirmed) throw {code:'warp_confirm_wireguard',detail:''};
      mode = 'warp'; sites.enabled = applications.enabled = false;
    }
    protocol = args.protocol;
  }
  sites.runtime_active = sites.enabled && installed && connected && mode === 'proxy' && protocol === 'MASQUE';
  applications.runtime_active = applications.enabled && installed && connected && mode === 'proxy' && protocol === 'MASQUE';
  if (command === 'check_warp_connection') {
    if (probeFailure === 'throw') throw {code:'warp_timeout',detail:'Mock health probe unavailable'};
    return { installed, connected, mode, protocol, proxy_available: mode === 'proxy' ? connected : null,
      route: probeFailure ? 'failed' : connected ? 'working' : 'unverified', error: probeFailure ? {code:'warp_check_failed',detail:'Mock route failed'} : null };
  }
  return {installed, connected, protocol, protocol_editable: protocolEditable, diag_supported: false, automation: structuredClone(automation), last_errors: [], sites: structuredClone(sites), applications: structuredClone(applications), state:connected ? 'connected':pendingConnection ? 'connecting':'disconnected', mode, modes, version:'warp-cli 2026.7.1343.0', proxy:mode === 'proxy' ? {address:'127.0.0.1',port,kind:'SOCKS5',active:connected,port_editable:true}:null};
}}};
setLanguage('ru');
updateStatusUI({running:true, strategy:'general (ALT12)'});
initStrategyDropdown();
await loadStrategies('general (ALT12)');
onLangChange(() => updateStatusUI({running:true, strategy:'general (ALT12)'}));
const hero = document.querySelector('#section-home > section');
const elements = [...hero.querySelectorAll('*'), hero];
const rects = () => elements.map(e => { const r=e.getBoundingClientRect(); return [r.x,r.y,r.width,r.height]; });
await document.fonts.ready;
const before = JSON.stringify(rects());
const loadingVisible = getComputedStyle($('startup-loader')).display === 'flex' && getComputedStyle($('sections-host')).visibility === 'hidden';
await initWarp();
initNavigation(); initTrafficMonitor();
const detectionFinished = $('warp-settings-status').textContent.includes('WARP');
document.documentElement.classList.remove('app-loading');
initInfoModals(); initStatusCheck();
await wait(200);
const results = [];
function check(condition, label) {results.push(`${condition?'PASS':'FAIL'}: ${label}`); if(!condition) console.error(label);}
check(loadingVisible && detectionFinished && startupBusy === 0 && getComputedStyle($('startup-loader')).display === 'none', 'Startup waits for detection even when the WARP client is busy');
for (const id of ['connect-btn', 'check-status-btn', 'status-modal-close', 'status-modal-done']) check(getComputedStyle($(id)).cursor === 'pointer', `${id} uses a pointer cursor`);
$('zapret-info-btn').click();
check(!$('info-modal').classList.contains('hidden') && $('info-content').textContent.includes('DPI'), 'Zapret help works without WARP');
$('info-modal-close').click();
async function manualCheck(id) {
  $(id).click();
  await wait(200);
  $('warp-status-dialog').close();
}
check(before === JSON.stringify(rects()), 'WARP absent: all original hero element bounds unchanged');
check($('warp-card').hidden, 'WARP absent: no visible WARP card');
check(!$('warp-install').hidden && !$('warp-install').disabled, 'Missing WARP offers installation in Settings');
// Lists are editable before installing or connecting WARP.
$('warp-sites-add').click(); $('warp-sites-input').value = 'offline.example';
$('warp-sites-dialog-add').click(); await wait(200);
check(sites.domains.includes('offline.example') && !connected, 'Website list saves with WARP disconnected and absent');
[...$('warp-sites-list').querySelectorAll('button')].find(b => b.getAttribute('aria-label').includes('offline.example')).click(); await wait(200);
installed = true; $('warp-settings-refresh').click();
check($('warp-status-dialog').open && $('warp-status-report').getAttribute('aria-busy') === 'true', 'Settings status check opens a loading report');
await wait(200);
check($('warp-status-report').textContent.includes('2026.7.1343.0'), 'Settings status check shows client version');
check(!$('warp-status-report').textContent.includes('Версия клиента'), 'CLI version appears under Client without a separate version row');
$('warp-status-dialog').close();
check(hero.classList.contains('has-warp') && !$('warp-card').hidden, 'Detection enables two cards');
check(getComputedStyle($('hero-status'), '::before').width === getComputedStyle($('warp-state'), '::before').width, 'Both provider badges show matching dots');
$('warp-info-btn').click();
check(!$('info-modal').classList.contains('hidden') && $('info-content').textContent.includes('Cloudflare'), 'WARP help opens');
$('info-modal-close').click();
$('check-status-btn').click(); await wait(100);
check(!$('status-modal').classList.contains('hidden') && $('status-content').textContent.includes('general (ALT12)'), 'Zapret status opens and renders its report');
check($('status-modal-title').textContent === 'Статус ZAPRET', 'Zapret status title names ZAPRET');
$('status-modal-close').click();
$('warp-connect').click();
check(!$('warp-connect').disabled && $('warp-connect-label').textContent === 'Отменить подключение', 'Pending connect can be cancelled with the same button');
await wait(20); $('warp-connect').click();
check($('warp-connect').disabled, 'Cancellation prevents repeated disconnect commands');
await wait(200);
check(!connected && calls.filter(c=>c==='disconnect_warp').length === 1, 'Cancellation disconnects after the pending CLI command');
pendingConnection = true; $('warp-connect').click(); await wait(200);
check(!$('warp-connect').disabled && $('warp-connect-label').textContent === 'Отменить подключение', 'An unestablished tunnel remains cancellable after CLI returns');
$('warp-connect').click(); await wait(200);
check(!connected && !pendingConnection, 'Cancellation stops an unestablished tunnel');
const connectionsBeforeModeChange = calls.filter(command => command === 'connect_warp').length;
for (const value of ['proxy', 'warp']) {
  $('warp-mode-trigger').click();
  $('warp-mode-options').querySelector(`[data-value="${value}"]`).click();
  if (!$('confirm-modal').classList.contains('hidden')) $('confirm-modal-ok').click();
  await wait(200);
  check(!connected && $('warp-connect').dataset.action === 'start', `Selecting ${value} leaves disconnected WARP off`);
}
check(calls.filter(command => command === 'connect_warp').length === connectionsBeforeModeChange, 'Changing mode does not issue a frontend connect command');
updateStatusUI({running:false}); await wait(350);
check(['connect-btn','warp-connect'].every(id => getComputedStyle($(id)).backgroundColor === 'rgba(0, 0, 0, 0)'), 'Disconnected connection buttons are both outlined');
updateStatusUI({running:true, strategy:'general (ALT12)'});
$('warp-connect').click(); await wait(350);
check(connected && $('connect-btn').dataset.action === 'stop', 'Both providers stay connected independently');
check(getComputedStyle($('connect-btn')).backgroundColor === getComputedStyle($('warp-connect')).backgroundColor, 'Connected buttons use the same fill');
for (const [left, right] of [['connect-btn', 'warp-connect'], ['check-status-btn', 'warp-refresh']]) {
  const a = $(left).getBoundingClientRect(), b = $(right).getBoundingClientRect();
  check((left !== 'connect-btn' || Math.abs(a.width - b.width) < 1) && a.height === b.height, `${left} and ${right} align; settings stays available beside WARP status`);
}
$('warp-mode-trigger').click();
const warpRow = $('warp-mode-options-list').querySelector('[aria-selected="true"]');
check(warpRow && warpRow.getBoundingClientRect().height > 0, 'Connection mode selector opens on the home page');
$('warp-mode-trigger').click();
check(!$('warp-card').textContent.includes('warp-cli'), 'Client version is absent from the home card');
check($('header-status').textContent.includes('general (ALT12) + WARP'), 'Header shows strategy plus WARP');
updateStatusUI({running:false});
check($('header-status').textContent.endsWith('WARP'), 'Header shows WARP alone when Zapret is stopped');
updateStatusUI({running:true, strategy:'general (ALT12)'});
$('warp-mode-trigger').click();
check(!$('warp-mode-options').hidden, 'Custom mode dropdown opens');
$('warp-mode-options').querySelector('[data-value="proxy"]').click();
if (!$('confirm-modal').classList.contains('hidden')) $('confirm-modal-ok').click();
await wait(200);
check($('warp-mode-options').hidden && $('warp-mode-label').textContent === 'Локальный прокси', 'Custom dropdown selects mode and closes');
check(!$('warp-proxy').hidden && $('warp-proxy-endpoint').textContent.includes('40000'), 'Local proxy reports endpoint and type');
$('warp-port').value = '40001'; $('warp-port-form').requestSubmit(); await wait(200);
check(port === 40001 && $('warp-proxy-endpoint').textContent.includes('40001'), 'Proxy port changes and refreshes');
$('warp-sites').open = true;
check($('warp-sites-domains').value === 'example.org', 'Saved website domains load from the backend');
$('warp-sites-add').click();
$('warp-sites-input').value = 'https://Example.net/path?source=test';
$('warp-sites-dialog-add').click(); await wait(200);
check(sites.domains.includes('example.net') && !$('warp-sites-domains').value.includes('https://'), 'Adding a website URL stores only its normalized domain');
$('warp-sites-domains').value = 'EXAMPLE.com\nexample.org';
$('warp-sites-domains').dispatchEvent(new Event('input'));
await manualCheck('warp-settings-refresh');
check($('warp-sites-domains').value.startsWith('EXAMPLE.com'), 'Polling preserves unsaved website edits');
$('warp-sites-toggle').click(); await wait(200);
check(sites.enabled && sites.domains[0] === 'example.com' && $('warp-port').disabled, 'Website rules enable with normalized domains and lock proxy port');
$('warp-sites-toggle').click(); await wait(200);
check(!sites.enabled && !$('warp-port').disabled && $('warp-sites-domains').value.includes('example.com'), 'Disabling website rules keeps the saved list and releases the port');
$('warp-sites').open = false;
$('warp-open-settings').click(); await wait(220);
check(!$('section-warp-settings').classList.contains('hidden') && $('section-home').classList.contains('hidden'), 'Home opens the dedicated WARP settings page');
check($('section-warp-settings').contains($('warp-sites')) && !$('warp-card').contains($('warp-sites')), 'Website rules moved off the home card');
check($('warp-sites-list').children.length === 2 && [...$('warp-sites-list').children].every(card => card.classList.contains('warp-rule-card')), 'Websites are rendered as separate compact cards');
$('warp-sites-info').click();
check($('warp-settings-info-dialog').open && $('warp-settings-info-text').textContent.includes('Сайты из списка'), 'Website explanation opens on demand');
$('warp-settings-info-dialog').close();
$('warp-apps-add').click(); await wait(250);
check(applications.paths.length === 1 && $('warp-apps-list').textContent.includes('LeagueClientUx.exe') && !$('warp-apps-list').textContent.includes('C:\\Games\\'), 'Application card shows only the executable name');
$('warp-apps-list').querySelector('[aria-label^="Полный путь"]').click();
check($('warp-settings-info-dialog').open && $('warp-settings-info-path').textContent.includes('C:\\Games\\League of Legends\\LeagueClientUx.exe'), 'Full application path opens on demand');
$('warp-settings-info-dialog').close();
$('warp-apps-toggle').click(); await wait(200);
check(applications.runtime_active && !$('warp-apps-add').disabled && $('warp-port').disabled, 'Application rules enable while list editing remains available');
$('warp-apps-toggle').click(); await wait(200);
check(!applications.enabled && !$('warp-apps-add').disabled && applications.paths.length === 1, 'Disabling application rules retains the selected executable');
$('warp-apps-list').querySelector('[aria-label^="Убрать"]').click(); await wait(200);
check(!applications.paths.length && !$('warp-apps-empty').hidden, 'Removing the last application shows the empty state');
showSection('home'); await wait(220);
check(!$('section-home').classList.contains('hidden') && $('section-warp-settings').classList.contains('hidden'), 'Connection controls return to the home page');
$('warp-refresh').click(); await wait(200);
check($('warp-status-dialog').open && $('warp-status-report').textContent.includes('127.0.0.1:40001'), 'Home status check reports proxy endpoint');
$('warp-status-dialog').close();
failed = true; $('warp-connect').click(); await wait(200);
check(!$('warp-error').hidden && !$('warp-connect').disabled, 'Operation failure is visible and retry available');
$('warp-settings-refresh').click(); await wait(200);
check($('warp-status-report').textContent.includes('Test failure'), 'Manual status check displays backend errors');
$('warp-status-dialog').close();
failed = false; mode = 'warp+doh'; connected = false;
await wait(3300);
check($('warp-mode').value === 'warp+doh' && $('warp-state').textContent.includes('Отключено'), 'External changes synchronize without clicking refresh');
installed = false; await manualCheck('warp-settings-refresh');
// Original Zapret buttons use a 300ms transition-all; let their legacy padding settle.
await wait(350);
check(!hero.classList.contains('has-warp') && $('warp-card').hidden, 'Uninstall restores original screen');
check(before === JSON.stringify(rects()), 'Uninstall restores original element bounds');
check(maximum === 1, 'Polling and operations never overlap');
installed = true; connected = true; mode='proxy'; await manualCheck('warp-settings-refresh');
setLanguage('en');
check($('warp-state').textContent === 'Connected' && $('warp-mode').selectedOptions[0].textContent === 'Local proxy', 'Dynamic labels change language');
setLanguage('ru'); mode='warp'; await manualCheck('warp-settings-refresh');
// Switch from a full tunnel through a single routing action and confirm the consequence.
connected = false; protocol = 'WireGuard'; mode = 'warp'; await manualCheck('warp-settings-refresh');
check(!$('warp-open-settings').hidden && !$('warp-sites-toggle').disabled, 'Settings and routing are accessible from a full-tunnel mode');
$('warp-sites-toggle').click();
check(!$('confirm-modal').classList.contains('hidden'), 'Changing a full tunnel requires confirmation');
$('confirm-modal-cancel').click(); await wait(50);
check(mode === 'warp' && protocol === 'WireGuard' && !connected, 'Cancellation preserves the official client mode and protocol');
$('warp-sites-toggle').click(); $('confirm-modal-ok').click(); await wait(250);
check(mode === 'proxy' && protocol === 'MASQUE' && sites.runtime_active, 'Confirmed activation prepares MASQUE proxy and starts website rules');
failed = true; await manualCheck('warp-settings-refresh');
check($('warp-sites-toggle').dataset.enabled === 'false' && $('warp-sites-state').dataset.state === 'error', 'An unavailable status never presents a stale active routing snapshot as verified');
failed = false; await manualCheck('warp-settings-refresh');
$('warp-apps-add').click(); await wait(250); $('warp-apps-toggle').click(); await wait(200);
$('warp-sites-toggle').click(); await wait(200);
check(!sites.runtime_active && applications.runtime_active && connected, 'Disabling sites preserves application routing and the WARP connection');
$('warp-apps-toggle').click(); await wait(200);
check(!applications.runtime_active && connected, 'Disabling application rules leaves WARP connected');
activationFailure = true; $('warp-sites-toggle').click(); await wait(350);
check($('warp-sites-toggle').dataset.enabled === 'false' && !$('warp-page-error').hidden && sites.domains.length > 0, 'Failed activation never shows a working toggle or loses the website list');
activationFailure = false; await manualCheck('warp-settings-refresh');
check($('warp-sites-state').textContent.includes('Настроено') && $('warp-sites-toggle').dataset.enabled === 'false', 'Persisted desired rules are displayed as configured, not active');
$('warp-sites-toggle').click(); await wait(200);
$('warp-auto-restore').click(); await wait(200);
check(automation.restore_rules && !Object.hasOwn(automation, 'connect_on_start'), 'Only the restore preference is saved; removed autoconnect cannot be re-enabled');
check($('warp-advanced').contains($('warp-auto-restore')) && $('warp-advanced').contains($('warp-view-traffic')), 'Restore and traffic link are inside advanced settings');
check(!$('warp-auto-connect') && !$('warp-check') && !$('warp-page-connect') && !$('warp-page-connection-title'), 'Connection panel and separate check button are removed');
$('warp-refresh').click(); await wait(200);
check($('warp-status-dialog').open && $('warp-status-report').textContent.includes('Тестовый маршрут через WARP') && $('warp-status-report').textContent.includes('Работает') && $('warp-status-report').textContent.includes('MASQUE'), 'WARP status dialog includes protocol, SOCKS5 probe and route result');
$('warp-status-dialog').close();
probeFailure = true; $('warp-refresh').click(); await wait(200);
check($('warp-status-report').textContent.includes('Mock route failed') && $('warp-status-report').textContent.includes('Не работает'), 'Failed route is explained inside the status dialog');
$('warp-status-dialog').close(); probeFailure = 'throw'; $('warp-refresh').click(); await wait(200);
check($('warp-status-report').textContent.includes('Mock health probe unavailable') && $('warp-status-report').textContent.includes('Не проверен'), 'A failed health command leaves the route explicitly unverified');
$('warp-status-dialog').close(); probeFailure = false;
protocol = null; await manualCheck('warp-settings-refresh');
check($('warp-protocol').value === '' && $('warp-protocol-label').textContent === 'Выберите протокол', 'Unknown protocol is a placeholder, never an invented selection');
$('warp-protocol-trigger').click();
check([...$('warp-protocol-options-list').querySelectorAll('button')].map(b => b.dataset.value).join(',') === 'MASQUE,WireGuard', 'Protocol dropdown contains only MASQUE and WireGuard');
$('warp-protocol-trigger').click();
protocol = 'MASQUE'; protocolEditable = false; await manualCheck('warp-settings-refresh');
check($('warp-protocol-trigger').disabled && !$('warp-protocol-hint').hidden, 'Unsupported protocol control is disabled with an explanation');
protocolEditable = true; await manualCheck('warp-settings-refresh');
for (const button of document.querySelectorAll('[data-warp-info]')) {
  button.click();
  check($('warp-settings-info-dialog').open && $('warp-settings-info-text').textContent.length > 40, `Information dialog describes ${button.dataset.warpInfo}`);
  $('warp-settings-info-dialog').close();
}
$('warp-view-traffic').click(); await wait(220);
check(!$('section-traffic').classList.contains('hidden') && document.querySelector('.traffic-mode-btn[data-mode="warp"]').getAttribute('aria-pressed') === 'true', 'WARP traffic link opens the existing monitor with the WARP filter');
check(!$('warp-connections') && !$('section-warp-settings').querySelector('.warp-experimental'), 'Settings contain neither duplicate connections monitor nor Experimental badge');
showSection('warp-settings'); await wait(220);
const report = document.createElement('pre'); report.id='test-results'; report.textContent=results.join('\n');
report.style.cssText='position:relative;margin:24px;padding:16px;background:#20232a;color:white;font:12px monospace;white-space:pre-wrap';
$('sections-host').append(report);
const params = new URLSearchParams(location.search);
if(params.get('width')) document.querySelector('body').style.width = `${params.get('width')}px`;
if(params.get('lang')) setLanguage(params.get('lang'));
if(params.get('theme')) document.querySelector(`input[name="theme-pref"][value="${params.get('theme')}"]`)?.click();
if(params.get('proxy')) {mode='proxy'; await manualCheck('warp-settings-refresh');}
if(params.get('sites')) {mode='proxy'; connected=true; applications.paths=['C:\\Games\\League of Legends\\LeagueClientUx.exe','C:\\Games\\League of Legends\\LeagueClientUxRender.exe']; await manualCheck('warp-settings-refresh'); showSection('warp-settings');}
if(params.get('settings')) { $('section-home').classList.add('hidden'); $('section-settings').classList.remove('hidden'); }

