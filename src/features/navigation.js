import { $ } from '../lib/core.js';
import { state } from '../lib/state.js';
import { showRestartModal } from '../lib/restart.js';
import { loadUserLists } from './user-lists.js';

const ALL_SECTIONS = ['section-home', 'section-sites', 'section-ips', 'section-diagnostics', 'section-traffic', 'section-settings', 'section-telegram', 'section-warp-settings'];

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
  indicator.style.top = (elRect.top - asideRect.top - aside.clientTop + aside.scrollTop) + 'px';
  indicator.style.height = elRect.height + 'px';
  indicator.style.opacity = '1';
}

export function initNavigation() {
  $('nav-warp-settings')?.addEventListener('click', e => { e.preventDefault(); showSection('warp-settings'); });
  $('nav-telegram')?.addEventListener('click', e => { e.preventDefault(); if (!$('nav-telegram').hidden) showSection('telegram'); });
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

  // Track layout changes as well as selection: maximizing the window moves
  // Settings, and installing/removing a module shifts the other navigation links.
  const aside = document.querySelector('aside');
  let frame = null;
  const syncIndicator = () => {
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      const active = aside?.querySelector('a.nav-active');
      if (active && !active.hidden) updateNavIndicator(active);
    });
  };
  window.addEventListener('resize', syncIndicator);
  if (aside) {
    const sizes = new ResizeObserver(syncIndicator);
    sizes.observe(aside);
    aside.querySelectorAll('nav, a').forEach(element => sizes.observe(element));
    const visibility = new MutationObserver(syncIndicator);
    visibility.observe(aside, { subtree: true, attributes: true, attributeFilter: ['hidden'], childList: true });
  }
  document.fonts.ready.then(syncIndicator);
  syncIndicator();
}
