import './styles.css';
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

import { ensureAdminPrivileges } from './features/admin-check.js';
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
import { initWizard } from './features/wizard.js';
import { initFirstRun, maybeShowFirstRun, openSetup } from './features/firstrun.js';
import { initStatusCheck } from './features/status-check.js';
import { initTrafficMonitor, refreshTrafficTranslations } from './features/traffic.js';
import { initWarp } from './features/warp.js';

// Mount HTML fragments synchronously so `[data-i18n]` elements are already in
// the DOM when Tailwind's CDN JIT observer and i18n engine run over them.
mountComponents();
initTheme();


window.addEventListener('DOMContentLoaded', async () => {
  initI18n();
  if (new URLSearchParams(location.search).get('setup') === '1') {
    document.documentElement.dataset.setupWindow = 'true';
    initFirstRun();
    const params = new URLSearchParams(location.search);
    openSetup({ repeat: params.get('repeat') === 'true', clearLists: params.get('clearLists') === 'true' });
    return;
  }
  document.addEventListener('contextmenu', (e) => e.preventDefault());

  initTitlebar();
  initWarp();

  const adminOk = await ensureAdminPrivileges();
  if (!adminOk) { await invoke('show_app_window', { force: true }); return; }

  initFirstRun();
  await maybeShowFirstRun();
  await invoke('show_app_window', { force: false });

  await loadCachedTestResults();
  await loadStrategies();
  initStrategyDropdown();
  initCustomStrategyImport();

  await loadVersions();

  await pollStatus();
  await pollFilters();
  await pollFakes();
  syncTrayLocalization();

  setInterval(async () => {
    if (state.setupActive) return;
    await pollStatus();
    await pollFilters();
    await pollFakes();
  }, 2000);

  initNavigation();
  initConnectButtons();
  initFilterButtons();
  initFakeSelectors();
  initUserLists();
  initInfoModals();
  initUpdates();
  initDiagnostics();
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

});
