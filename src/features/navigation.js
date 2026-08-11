import { $ } from '../lib/core.js';
import { state } from '../lib/state.js';
import { showRestartModal } from '../lib/restart.js';
import { loadUserLists } from './user-lists.js';

const ALL_SECTIONS = ['section-home', 'section-sites', 'section-ips', 'section-diagnostics', 'section-traffic', 'section-settings'];

export function showSection(sectionId) {
  if (state.pendingRestart && !state.restartGuardDismissed && sectionId !== state.currentSectionId) {
    state.pendingNavId = sectionId;
    showRestartModal();
    return;
  }
  if (sectionId === state.currentSectionId) return;

  // Update nav immediately (feels responsive)
  document.querySelectorAll('aside a').forEach((a) => {
    a.classList.remove('nav-active');
    a.classList.add('nav-inactive');
  });
  const activeNav = sectionId === 'home' ? document.querySelector('aside nav a:first-child') : $(`nav-${sectionId}`);
  if (activeNav) {
    activeNav.classList.remove('nav-inactive');
    activeNav.classList.add('nav-active');
    updateNavIndicator(activeNav);
  }

  const prevId = state.currentSectionId;
  state.currentSectionId = sectionId;
  document.dispatchEvent(new CustomEvent('zapret:section-changed', { detail: { sectionId } }));

  const prevSection = prevId ? $(`section-${prevId}`) : null;
  const nextSection = $(`section-${sectionId}`);

  if (prevSection && !prevSection.classList.contains('hidden')) {
    // Fade out current, then swap
    prevSection.classList.add('section-exit');
    setTimeout(() => {
      prevSection.classList.add('hidden');
      prevSection.classList.remove('section-exit');
      if (nextSection) showWithAnim(nextSection);
    }, 120); // matches secFadeOut duration
  } else {
    // No previous visible section — just show immediately
    ALL_SECTIONS.forEach((id) => $(id)?.classList.add('hidden'));
    if (nextSection) showWithAnim(nextSection);
  }
}

/** Show a section with the enter animation, then remove the class so
 *  CSS transform from 'forwards' fill mode doesn't break fixed-position children. */
function showWithAnim(el) {
  el.classList.remove('hidden');
  el.classList.remove('section-enter');
  void el.offsetWidth; // force reflow so animation restarts cleanly
  el.classList.add('section-enter');
  el.addEventListener('animationend', () => {
    el.classList.remove('section-enter'); // removes transform:translateY(0) from forwards fill
  }, { once: true });
}

function updateNavIndicator(activeEl) {
  const indicator = $('nav-indicator');
  const aside = document.querySelector('aside');
  if (!indicator || !activeEl || !aside) return;
  const asideRect = aside.getBoundingClientRect();
  const elRect = activeEl.getBoundingClientRect();
  indicator.style.top = (elRect.top - asideRect.top) + 'px';
  indicator.style.height = elRect.height + 'px';
  indicator.style.opacity = '1';
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
  $('nav-traffic')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('traffic');
  });
  $('nav-settings')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('settings');
  });

  // Set indicator to initial active item after layout
  requestAnimationFrame(() => {
    const firstNav = document.querySelector('aside nav a:first-child');
    if (firstNav) updateNavIndicator(firstNav);
  });
}
