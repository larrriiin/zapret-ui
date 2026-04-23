// Each section and modal lives in its own HTML file. We import them as raw
// strings via Vite's `?raw` suffix so they are inlined at build time — no
// extra HTTP requests at runtime — and mount them synchronously before the
// rest of `main.js` initialises the app.

import sectionHome from './sections/home.html?raw';
import sectionSites from './sections/sites.html?raw';
import sectionIps from './sections/ips.html?raw';
import sectionDiagnostics from './sections/diagnostics.html?raw';

import modalCloseConfirm from './modals/close-confirm.html?raw';
import modalRestart from './modals/restart.html?raw';
import modalUpdate from './modals/update.html?raw';
import modalLatestVersion from './modals/latest-version.html?raw';
import modalFirstLaunch from './modals/first-launch.html?raw';
import modalWizard from './modals/wizard.html?raw';
import modalStrategiesFirstrun from './modals/strategies-firstrun.html?raw';
import modalStatus from './modals/status.html?raw';
import modalInfoHelp from './modals/info-help.html?raw';

import restartBanner from './restart-banner.html?raw';

function mount(hostId, html) {
  const host = document.getElementById(hostId);
  if (!host) {
    console.warn(`mount: missing host element #${hostId}`);
    return;
  }
  host.insertAdjacentHTML('beforeend', html);
}

export function mountComponents() {
  // Sections go inside <main> so sidebar/header layout stays intact.
  mount('sections-host', sectionHome);
  mount('sections-host', sectionSites);
  mount('sections-host', sectionIps);
  mount('sections-host', sectionDiagnostics);

  // Modals and the restart banner are top-level, mounted into <body>.
  mount('modals-host', modalCloseConfirm);
  mount('modals-host', modalRestart);
  mount('modals-host', modalUpdate);
  mount('modals-host', modalLatestVersion);
  mount('modals-host', modalFirstLaunch);
  mount('modals-host', modalWizard);
  mount('modals-host', modalStrategiesFirstrun);
  mount('modals-host', modalStatus);
  mount('modals-host', modalInfoHelp);
  mount('modals-host', restartBanner);
}
