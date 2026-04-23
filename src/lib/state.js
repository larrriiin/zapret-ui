// Shared mutable app state. Exported as a single object so features can read
// and write without relying on globals on `window`. Fields mirror the
// pre-Vite main.js globals so the refactor is behaviour-preserving.
export const state = {
  currentFilters: { game_filter: 'disabled', ipset: 'any' },
  previousGameFilter: 'all',
  previousIPSet: 'any',
  pendingRestart: false,
  restartGuardDismissed: false,
  currentSectionId: 'home',
  pendingNavId: null,
  cachedTestResults: null,
};
