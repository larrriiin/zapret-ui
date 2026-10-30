import '/styles.css';
import { mountComponents } from '/components/index.js';
import { initI18n, setLanguage } from '/lib/i18n.js';
import { initTheme, syncThemeFromStorage } from '/features/theme.js';
import { initNavigation, showSection } from '/features/navigation.js';
import { initInfoModals } from '/features/info-modals.js';
import { initTitlebar } from '/features/titlebar.js';
import { initAutomations } from '/features/automations.js';
mountComponents();
const params = new URLSearchParams(location.search);
localStorage.setItem('zapret_theme', params.get('theme') || 'zapret');
syncThemeFromStorage(); initTheme(); initI18n();
let config = { enabled: true, rules: [] };
window.testFailSave = false;
window.testActive = [];
window.__TAURI__ = { core: { invoke: async (command, args) => {
  if (command === 'get_application_rules') return { config: structuredClone(config), active: window.testActive, paused: false, error: null };
  if (command === 'save_application_rules') {
    if (window.testFailSave) throw 'Save failed';
    config = structuredClone(args.config); window.testConfig = config; return;
  }
  if (command === 'get_strategies') return ['general', 'general (ALT12)'];
  if (command === 'get_warp_status') return { modes: ['warp', 'proxy', 'doh', 'dot', 'warp+dot', 'warp+doh', 'tunnel_only'], sites: { domains: ['example.org', 'video.example'] }, applications: { paths: ['C:\\Apps\\Discord.exe', 'C:\\Apps\\Browser.exe'] } };
  if (command === 'choose_rule_application') return 'C:\\Games\\Example\\game.exe';
  return null;
} } };
window.testWindowCalls = [];
let maximized = false, resizeHandler;
window.__TAURI__.window = { getCurrentWindow: () => ({
  minimize: async () => window.testWindowCalls.push('minimize'),
  close: async () => window.testWindowCalls.push('close'),
  isMaximized: async () => maximized,
  toggleMaximize: async () => { maximized = !maximized; window.testWindowCalls.push('maximize'); resizeHandler?.(); },
  onResized: handler => { resizeHandler = handler; },
}) };
initTitlebar();
setLanguage(params.get('lang') || 'ru');
initNavigation(); initInfoModals(); initAutomations(); showSection('automations');
document.documentElement.classList.remove('app-loading');
