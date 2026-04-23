import { $ } from '../lib/core.js';
import { state } from '../lib/state.js';
import { showRestartModal } from '../lib/restart.js';
import { loadUserLists } from './user-lists.js';

export function showSection(sectionId) {
  if (state.pendingRestart && !state.restartGuardDismissed && sectionId !== state.currentSectionId) {
    state.pendingNavId = sectionId;
    showRestartModal();
    return;
  }
  if (sectionId === state.currentSectionId) return;

  state.currentSectionId = sectionId;

  ['section-home', 'section-sites', 'section-ips', 'section-diagnostics'].forEach((id) => {
    $(id)?.classList.add('hidden');
  });
  const section = $(`section-${sectionId}`);
  if (section) section.classList.remove('hidden');

  document.querySelectorAll('aside nav a').forEach((a) => {
    a.classList.remove('border-r-2', 'border-[#ba9eff]', 'bg-gradient-to-r', 'from-[#ba9eff]/10', 'to-transparent', 'text-[#ba9eff]');
    a.classList.add('text-[#dfe4fe]/40');
  });

  const activeNav = sectionId === 'home' ? document.querySelector('aside nav a:first-child') : $(`nav-${sectionId}`);
  if (activeNav) {
    activeNav.classList.remove('text-[#dfe4fe]/40');
    activeNav.classList.add('border-r-2', 'border-[#ba9eff]', 'bg-gradient-to-r', 'from-[#ba9eff]/10', 'to-transparent', 'text-[#ba9eff]');
  }
}

export function initNavigation() {
  document.querySelector('aside nav a:first-child')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('home');
  });
  $('nav-sites')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('sites');
    loadUserLists();
  });
  $('nav-ips')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('ips');
    loadUserLists();
  });
  $('nav-diagnostics')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('diagnostics');
  });
}
