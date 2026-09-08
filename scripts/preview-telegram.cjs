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
localStorage.setItem('zapret_lang', 'ru');
const params = new URLSearchParams(location.search);
let installed = params.has('installed'), running = false, warp = params.has('warp');
let fail = params.has('fail');
let port = 1443;
const calls = [];
const handlers = {};
window.__TAURI__ = { core: { invoke: async (cmd, args) => {
  calls.push(cmd);
  document.body.dataset.calls = JSON.stringify(calls);
  if (cmd === 'get_telegram_status') return { installed, running, busy: false, version: '1.10.2', download_bytes: 16923850, port };
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
}}, event: {listen: async (name, fn) => { handlers[name] = fn; return () => delete handlers[name]; }}, window: { getCurrentWindow: () => ({minimize: async () => {}}) }};
mountComponents(); initTheme(); initI18n();
document.documentElement.classList.remove('app-loading');
initNavigation();
if (params.has('setup')) { document.documentElement.dataset.setupWindow = 'true'; initFirstRun(); openSetup(); }
else { initTelegram(); showSection('settings'); }
`);
console.log('Preview: http://127.0.0.1:1420/@fs/' + folder.replaceAll('\\', '/') + '/preview.html');
