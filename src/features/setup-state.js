export const SETUP_COMPLETE_KEY = 'zapret.setup.completed';
const LEGACY_DISMISSED_KEY = 'zapret.firstrun.dismissed';

function availableStorage(storage) {
  if (storage) return storage;
  try { return globalThis.localStorage; }
  catch { return null; }
}

export function markSetupCompleted(storage) {
  const target = availableStorage(storage);
  if (!target) return;
  try {
    target.setItem(SETUP_COMPLETE_KEY, '1');
    target.setItem(LEGACY_DISMISSED_KEY, '1');
  } catch { /* The installed core remains the fallback source of truth. */ }
}

export async function shouldShowFirstRun({ ensureBinaries, storage, warn = console.warn }) {
  const target = availableStorage(storage);
  let completed = false;
  try { completed = target?.getItem(SETUP_COMPLETE_KEY) === '1'; }
  catch { /* Validate the installed core instead. */ }

  try {
    if (await ensureBinaries()) {
      if (!completed) markSetupCompleted(target);
      return false;
    }
  } catch (error) {
    warn('Core check failed:', error);
    // A transient validation error must not trap an already configured user in
    // setup on every launch. Explicit setup remains available from Settings.
    if (completed) return false;
  }
  return true;
}
