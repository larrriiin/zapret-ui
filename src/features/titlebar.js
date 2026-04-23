import { $, invoke, getCurrentWindow } from '../lib/core.js';

const CLOSE_PREF_KEY = 'zapret.closePref'; // 'ask' | 'tray' | 'exit'
const getClosePref = () => localStorage.getItem(CLOSE_PREF_KEY) || 'ask';
const setClosePref = (v) => localStorage.setItem(CLOSE_PREF_KEY, v);

export function initTitlebar() {
  try {
    const win = getCurrentWindow();
    const tbMin = $('tb-minimize');
    const tbMax = $('tb-maximize');
    const tbMaxIcon = $('tb-maximize-icon');
    const tbClose = $('tb-close');
    const tbCloseMenu = $('tb-close-menu');
    const closeModal = $('close-confirm-modal');

    // Use win.close() (not win.hide()) so the Rust-side CloseRequested handler
    // runs and makes the tray icon visible + shows the "minimized" notification.
    const doMinimizeToTray = () => { try { win.close(); } catch {} };
    const doExit = () => invoke('exit_app').catch((err) => console.error('exit_app failed:', err));

    const updateMaximizeIcon = async () => {
      if (!tbMaxIcon) return;
      try {
        const isMax = await win.isMaximized();
        tbMaxIcon.textContent = isMax ? 'filter_none' : 'crop_square';
      } catch {}
    };

    tbMin?.addEventListener('click', () => win.minimize());
    tbMax?.addEventListener('click', async () => {
      try { await win.toggleMaximize(); } catch (e) { console.warn(e); }
      updateMaximizeIcon();
    });
    try { win.onResized(() => updateMaximizeIcon()); } catch {}
    updateMaximizeIcon();

    const hideCloseMenu = () => tbCloseMenu?.classList.add('hidden');
    document.addEventListener('click', (e) => {
      if (tbCloseMenu && !tbCloseMenu.contains(e.target) && e.target !== tbClose) hideCloseMenu();
    });

    const showCloseModal = () => {
      if (!closeModal) { doMinimizeToTray(); return; }
      const remember = $('close-confirm-remember');
      if (remember) remember.checked = false;
      closeModal.classList.remove('hidden');
    };

    if (tbClose) {
      tbClose.addEventListener('click', () => {
        hideCloseMenu();
        const pref = getClosePref();
        if (pref === 'exit') doExit();
        else if (pref === 'tray') doMinimizeToTray();
        else showCloseModal();
      });
      tbClose.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        if (!tbCloseMenu) return;
        const rect = tbClose.getBoundingClientRect();
        tbCloseMenu.style.top = rect.bottom + 2 + 'px';
        tbCloseMenu.style.right = window.innerWidth - rect.right + 'px';
        tbCloseMenu.classList.remove('hidden');
      });
    }

    $('tb-close-menu-tray')?.addEventListener('click', () => { hideCloseMenu(); doMinimizeToTray(); });
    $('tb-close-menu-exit')?.addEventListener('click', () => { hideCloseMenu(); doExit(); });

    const syncClosePrefRadios = () => {
      const pref = getClosePref();
      document.querySelectorAll('input[name="close-pref"]').forEach((r) => {
        r.checked = r.value === pref;
      });
    };

    const applyChoice = (choice) => {
      const remember = $('close-confirm-remember');
      if (remember?.checked) {
        setClosePref(choice);
        syncClosePrefRadios();
      }
      closeModal?.classList.add('hidden');
      if (choice === 'exit') doExit();
      else doMinimizeToTray();
    };
    $('close-confirm-tray')?.addEventListener('click', () => applyChoice('tray'));
    $('close-confirm-exit')?.addEventListener('click', () => applyChoice('exit'));
    $('close-confirm-cancel')?.addEventListener('click', () => closeModal?.classList.add('hidden'));

    // Settings popover + autostart toggle + close-pref radios
    const settingsBtn = $('settings-btn');
    const settingsPopover = $('settings-popover');
    if (settingsBtn && settingsPopover) {
      settingsBtn.addEventListener('click', (e) => {
        e.stopPropagation();
        settingsPopover.classList.toggle('hidden');
      });
      document.addEventListener('click', (e) => {
        if (!settingsPopover.contains(e.target) && e.target !== settingsBtn) {
          settingsPopover.classList.add('hidden');
        }
      });
    }

    syncClosePrefRadios();
    document.querySelectorAll('input[name="close-pref"]').forEach((r) => {
      r.addEventListener('change', () => { if (r.checked) setClosePref(r.value); });
    });

    const autostartToggle = $('autostart-toggle');
    const syncAutostartUI = async () => {
      if (!autostartToggle) return;
      try {
        const enabled = await invoke('plugin:autostart|is_enabled');
        autostartToggle.classList.toggle('is-on', !!enabled);
        autostartToggle.classList.toggle('is-off', !enabled);
      } catch (err) {
        console.warn('autostart is_enabled failed:', err);
      }
    };
    if (autostartToggle) {
      autostartToggle.addEventListener('click', async () => {
        try {
          const enabled = await invoke('plugin:autostart|is_enabled');
          await invoke(enabled ? 'plugin:autostart|disable' : 'plugin:autostart|enable');
        } catch (err) {
          console.error('autostart toggle failed:', err);
        }
        syncAutostartUI();
      });
    }
    syncAutostartUI();
  } catch (e) {
    console.warn('Title bar controls unavailable:', e);
  }
}
