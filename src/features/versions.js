import { $, invoke } from '../lib/core.js';

const VERSION_DISPLAYS = ['version-display', 'latest-version-number', 'update-current-version'];

export async function loadVersions() {
  try {
    const localVersion = await invoke('get_local_version_cmd');
    const uiVersion = await invoke('get_ui_version_cmd');

    VERSION_DISPLAYS.forEach((id) => {
      const el = $(id);
      if (el) el.textContent = id === 'version-display' ? 'v' + localVersion : localVersion;
    });

    const uiEl = $('ui-version-display');
    if (uiEl) uiEl.textContent = 'v' + uiVersion;
  } catch (e) {
    console.error('Failed to get versions:', e);
  }
}

export async function refreshCoreVersion() {
  try {
    const refreshedVersion = await invoke('get_local_version_cmd');
    VERSION_DISPLAYS.forEach((id) => {
      const el = $(id);
      if (el) el.textContent = id === 'version-display' ? 'v' + refreshedVersion : refreshedVersion;
    });
  } catch (e) {
    console.error('Failed to refresh version:', e);
  }
}
