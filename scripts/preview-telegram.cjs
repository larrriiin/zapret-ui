// Isolated browser fixture: all native commands are mocked, no software installs.
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const folder = path.join(root, 'artifacts/tg-smoke');
fs.mkdirSync(folder, { recursive: true });
const shell = fs.readFileSync(path.join(root, 'src/index.html'), 'utf8');
fs.writeFileSync(path.join(folder, 'preview.html'), shell.replace('<script type="module" src="/main.js"></script>', '<script type="module" src="./preview.js"></script>'));
fs.writeFileSync(path.join(folder, 'preview.js'), `
import '/styles.css';
import { mountComponents } from '/components/index.js';
import { initI18n } from '/lib/i18n.js';
import { initTheme } from '/features/theme.js';
import { initTelegram } from '/features/telegram.js';
import { initFirstRun, openSetup } from '/features/firstrun.js';
import { initNavigation, showSection } from '/features/navigation.js';
import { checkForUpdates } from '/features/updates.js';
localStorage.setItem('zapret_lang', 'ru');
const params = new URLSearchParams(location.search);
let installed = params.has('installed'), running = false, warp = params.has('warp');
let fail = params.has('fail');
let port = 1443;
let version = params.has('update') ? '1.10.1' : '1.10.2';
let config = { host: '127.0.0.1', port, secret: '00000000000000000000000000000000', dc_ips: {'2':'149.154.167.220','4':'149.154.167.220'}, cfproxy: false, cfproxy_domains: [], worker: false, worker_domains: [] };
const calls = [];
const handlers = {};
window.__TAURI__ = { core: { invoke: async (cmd, args) => {
  calls.push(cmd);
  document.body.dataset.calls = JSON.stringify(calls);
  if (cmd === 'get_telegram_status') return { installed, running, busy: false, version, download_bytes: 16923850, port:config.port, host:config.host };
  if (cmd === 'get_telegram_config') return structuredClone(config);
  if (cmd === 'save_telegram_config') { config = structuredClone(args.settings); return config; }
  if (cmd === 'regenerate_telegram_secret') { config.secret = '11111111111111111111111111111111'; return structuredClone(config); }
  if (cmd === 'check_telegram_update') return {installed, current:version, latest:'1.10.2', available:version !== '1.10.2'};
  if (cmd === 'update_telegram') { version = '1.10.2'; return version; }
  if (cmd === 'get_ui_version_cmd') return '26.9.12';
  if (cmd === 'get_core_update_info') return {status:'up_to_date', currentVersion:'1.0.0', stableVersion:'1.0.0'};
  if (cmd === 'get_warp_status') return { installed: warp };
  if (cmd === 'install_telegram') { if (fail) { fail = false; throw 'tg_checksum_failed'; } installed = true; }
  if (cmd === 'remove_telegram') { installed = false; running = false; }
  if (cmd === 'start_telegram') running = true;
  if (cmd === 'stop_telegram') running = false;
  if (cmd === 'set_telegram_port') port = args.port;
  if (cmd === 'install_warp') warp = true;
  if (cmd === 'telegram_logs') return 'INFO: Telegram MTProto WS Bridge Proxy\\nINFO: Secret: [secret]';
  if (cmd === 'precheck_tests') return { strategies_count: 1, is_admin: true };
  if (cmd === 'get_strategies') return ['general'];
  if (cmd === 'ensure_binaries_present') return true;
  return null;
}}, updater: {check: async () => null}, event: {listen: async (name, fn) => { handlers[name] = fn; return () => delete handlers[name]; }}, window: { getCurrentWindow: () => ({minimize: async () => {}}) }};
mountComponents(); initTheme(); initI18n();
document.documentElement.classList.remove('app-loading');
initNavigation();
if (params.has('setup')) { document.documentElement.dataset.setupWindow = 'true'; initFirstRun(); openSetup(); }
else { initTelegram(); showSection('settings'); document.getElementById('check-updates-btn').addEventListener('click', () => checkForUpdates(true)); if (params.has('update')) checkForUpdates(false); }
`);
console.log('Preview: http://127.0.0.1:1420/@fs/' + folder.replaceAll('\\', '/') + '/preview.html');
