import './styles.css';
import globeUrl from './assets/globe.png';
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
import { ensureBinariesPresent } from './features/binaries.js';
import { loadVersions } from './features/versions.js';
import { initTitlebar } from './features/titlebar.js';
import { initNavigation, showSection } from './features/navigation.js';
import { initConnectButtons } from './features/connect.js';
import { initFilterButtons, pollFilters } from './features/filters.js';
import {
  initStrategyDropdown,
  loadStrategies,
  loadCachedTestResults,
  renderStrategyList,
} from './features/strategies.js';
import { pollStatus } from './features/status.js';
import { initUserLists } from './features/user-lists.js';
import { initInfoModals, refreshOpenInfoModal } from './features/info-modals.js';
import { initUpdates } from './features/updates.js';
import { initDiagnostics } from './features/diagnostics.js';
import { initWizard } from './features/wizard.js';
import { initFirstRun, maybeShowFirstRun } from './features/firstrun.js';
import { initStatusCheck } from './features/status-check.js';

// Mount HTML fragments synchronously so `[data-i18n]` elements are already in
// the DOM when Tailwind's CDN JIT observer and i18n engine run over them.
mountComponents();

// Dynamically assign the built asset URL to the globe image so Vite compiles it
const globeImg = $('globe-image');
if (globeImg) {
  globeImg.src = globeUrl;
}

window.addEventListener('DOMContentLoaded', async () => {
  initI18n();
  document.addEventListener('contextmenu', (e) => e.preventDefault());

  initTitlebar();

  const adminOk = await ensureAdminPrivileges();
  if (!adminOk) return;

  await loadCachedTestResults();
  await loadStrategies();
  initStrategyDropdown();

  const binariesReady = await ensureBinariesPresent();
  if (!binariesReady) return;

  await loadVersions();

  await pollStatus();
  await pollFilters();
  syncTrayLocalization();

  setInterval(async () => {
    await pollStatus();
    await pollFilters();
  }, 2000);

  initNavigation();
  initConnectButtons();
  initFilterButtons();
  initUserLists();
  initInfoModals();
  initUpdates();
  initDiagnostics();
  initWizard();
  initFirstRun();
  initStatusCheck();

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
    renderStrategyList();
    refreshOpenInfoModal();
  });

  await maybeShowFirstRun();
});
