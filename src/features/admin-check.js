import { $, invoke, getProcess } from '../lib/core.js';

// Mirrors the pre-refactor guard. Returns true when the app should continue
// booting, false when admin rights are missing and the user should be shown
// the admin-check modal (if present in the DOM).
export async function ensureAdminPrivileges() {
  try {
    const isAdmin = await invoke('check_admin_privileges');
    if (!isAdmin) {
      $('admin-check-modal')?.classList.remove('hidden');
      $('admin-check-close')?.addEventListener('click', () => {
        getProcess()?.exit(1);
      });
      return false;
    }
  } catch (err) {
    console.error('Failed to check admin privileges:', err);
  }
  return true;
}
