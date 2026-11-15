import './styles.css';
import { initAutomations } from './features/automations.js';
import { initTheme } from './features/theme.js';
import { mountComponents } from './components/index.js';
import { $, invoke } from './lib/core.js';
import { initI18n, toggleLanguage, setLanguage, onLangChange, syncTrayLocalization } from './lib/i18n.js';
import { state } from './lib/state.js';
import {
  updateRestartBanner,
  hideRestartModal,
  restartServiceIfRunning,
} from './lib/restart.js';

import { initAdminPrivileges } from './features/admin-check.js';
import { loadVersions } from './features/versions.js';
import { initTitlebar } from './features/titlebar.js';
import { initNavigation, showSection } from './features/navigation.js';
import { initConnectButtons } from './features/connect.js';
import { initFilterButtons, pollFilters } from './features/filters.js';
import { initFakeSelectors, pollFakes } from './features/fakes.js';
import {
  initStrategyDropdown,
  initCustomStrategyImport,
  loadStrategies,
  loadCachedTestResults,
  renderStrategyList,
  refreshCustomStrategyImportTranslation,
} from './features/strategies.js';
import { pollStatus } from './features/status.js';
import { initUserLists } from './features/user-lists.js';
import { initInfoModals, refreshOpenInfoModal } from './features/info-modals.js';
import { initUpdates } from './features/updates.js';
import { initDiagnostics } from './features/diagnostics.js';
import { initHosts } from './features/hosts.js';
import { initWizard } from './features/wizard.js';
import { initFirstRun, maybeShowFirstRun, openSetup } from './features/firstrun.js';
import { initStatusCheck } from './features/status-check.js';
import { initTrafficMonitor, refreshTrafficTranslations } from './features/traffic.js';
import { initTelegram } from './features/telegram.js';
import { initWarp } from './features/warp.js';

// Mount HTML fragments before initializing translations and controls.
mountComponents();
initTheme();


window.addEventListener('DOMContentLoaded', async () => {
  try {
  initI18n();
  initAdminPrivileges();
  const elevationContext = await invoke('elevation_context');
  if (new URLSearchParams(location.search).get('setup') === '1') {
    document.documentElement.dataset.setupWindow = 'true';
    initFirstRun();
    const params = new URLSearchParams(location.search);
    openSetup({ repeat: params.get('repeat') === 'true', clearLists: params.get('clearLists') === 'true' });
    document.documentElement.classList.remove('app-loading');
    return;
  }
  document.addEventListener('contextmenu', (e) => e.preventDefault());

  initTitlebar();
  initWarp();

  initFirstRun();
  const resumeSetup = elevationContext?.section === 'setup';
  const forceShow = resumeSetup || await maybeShowFirstRun();
  if (resumeSetup) await openSetup({ repeat: elevationContext.repeat, clearLists: elevationContext.clear_lists, restoreMain: false });

  await loadCachedTestResults();
  await loadStrategies();
  initStrategyDropdown();
  initCustomStrategyImport();

  await loadVersions();

  await pollStatus();
  await pollFilters();
  await pollFakes();
  syncTrayLocalization();

  async function pollHome() {
    if (!state.setupActive) {
      await pollStatus();
      await pollFilters();
      await pollFakes();
    }
    setTimeout(pollHome, 2000);
  }
  setTimeout(pollHome, 2000);

  initNavigation();
  initAutomations();
  initTelegram();
  initConnectButtons();
  initFilterButtons();
  initFakeSelectors();
  initUserLists();
  initInfoModals();
  initUpdates();
  initDiagnostics();
  initHosts();
  initWizard();
  initStatusCheck();
  initTrafficMonitor();

  // Global restart-related buttons (live in top-level modals/banner).
  document.querySelectorAll('input[name="lang-pref"]').forEach(radio => {
    radio.addEventListener('change', (e) => {
      if (e.target.checked) setLanguage(e.target.value);
    });
  });

  $('restart-later')?.addEventListener('click', () => {
    hideRestartModal();
    state.restartGuardDismissed = true;
    const lastNavId = state.pendingNavId;
    if (lastNavId) {
      state.pendingNavId = null;
      showSection(lastNavId);
    }
  });
  $('restart-now')?.addEventListener('click', async () => {
    hideRestartModal();
    if (state.pendingRestart) {
      await restartServiceIfRunning();
      state.pendingRestart = false;
      updateRestartBanner();
    }
    const lastNavId = state.pendingNavId;
    if (lastNavId) {
      state.pendingNavId = null;
      showSection(lastNavId);
    }
  });
  $('restart-banner-btn')?.addEventListener('click', async () => {
    if (state.pendingRestart) {
      await restartServiceIfRunning();
      state.pendingRestart = false;
      updateRestartBanner();
    }
  });

  $('update-later')?.addEventListener('click', () => $('update-modal')?.classList.add('hidden'));
  $('latest-version-ok')?.addEventListener('click', () => $('latest-version-modal')?.classList.add('hidden'));

  // Refresh dynamic UI when language changes.
  onLangChange(() => {
    pollStatus();
    pollFilters();
    pollFakes();
    renderStrategyList();
    refreshCustomStrategyImportTranslation();
    refreshOpenInfoModal();
    refreshTrafficTranslations();
  });

  await document.fonts.ready;
  const home = $('section-home').getBoundingClientRect();
  const content = $('sections-host').getBoundingClientRect();
  await invoke('fit_initial_window_height', { height: Math.ceil(content.top + home.height) })
    .catch(error => console.warn('Could not fit the initial window height:', error));
  if (elevationContext && elevationContext.section !== 'setup') showSection(elevationContext.section);
  document.documentElement.classList.remove('app-loading');
  await invoke('show_app_window', { force: forceShow });
  } catch (error) {
    console.error('Startup failed:', error);
    document.getElementById('startup-spinner').hidden = true;
    document.getElementById('startup-message').textContent = document.documentElement.lang === 'ru' ? 'Не удалось загрузить приложение' : 'Unable to load the app';
    document.getElementById('startup-retry').hidden = false;
    document.getElementById('startup-retry').onclick = () => location.reload();
    await invoke('show_app_window', { force: false });
  }
});
