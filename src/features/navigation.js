import { $ } from '../lib/core.js';
import { state } from '../lib/state.js';
import { showRestartModal } from '../lib/restart.js';
import { loadUserLists } from './user-lists.js';

const ALL_SECTIONS = ['section-home', 'section-zapret-settings', 'section-traffic', 'section-settings', 'section-telegram', 'section-warp-settings'];
const ZAPRET_PANES = ['sites', 'ips', 'diagnostics'];

function normalizeSectionId(sectionId) {
  return ZAPRET_PANES.includes(sectionId) ? 'zapret-settings' : sectionId;
}

export function showSection(sectionId) {
  const requestedPane = ZAPRET_PANES.includes(sectionId) ? sectionId : null;
  sectionId = normalizeSectionId(sectionId);
  if (state.pendingRestart && !state.restartGuardDismissed && sectionId !== state.currentSectionId) {
    state.pendingNavId = sectionId;
    showRestartModal();
    return;
  }
  if (sectionId === state.currentSectionId) {
    if (requestedPane) showZapretSettingsPane(requestedPane);
    return;
  }

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

  if (sectionId === 'zapret-settings') {
    showZapretSettingsPane(requestedPane || 'ips');
    loadUserLists();
  }

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

export function showZapretSettingsPane(pane) {
  if (!ZAPRET_PANES.includes(pane)) return;
  ZAPRET_PANES.forEach((name) => {
    const paneElement = $(`zapret-settings-${name}`);
    paneElement?.classList.toggle('hidden', name !== pane);
    const tab = document.querySelector(`[data-zapret-pane="${name}"]`);
    if (tab) tab.setAttribute('aria-selected', String(name === pane));
  });
}

function nestZapretSettingsPanes() {
  const content = $('zapret-settings-content');
  if (!content || content.children.length) return;

  ZAPRET_PANES.forEach((pane) => {
    const section = $(`section-${pane}`);
    if (!section) return;
    section.id = `zapret-settings-${pane}`;
    section.className = 'zapret-settings-pane';
    section.firstElementChild?.classList.add('zapret-settings-pane-content');
    content.append(section);
  });
  showZapretSettingsPane('ips');
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
  nestZapretSettingsPanes();
  $('nav-warp-settings')?.addEventListener('click', e => { e.preventDefault(); showSection('warp-settings'); });
  $('nav-telegram')?.addEventListener('click', e => { e.preventDefault(); if (!$('nav-telegram').hidden) showSection('telegram'); });
  document.querySelector('aside nav a:first-child')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('home');
  });
  $('nav-zapret-settings')?.addEventListener('click', (e) => {
    e.preventDefault();
    showSection('zapret-settings');
  });
  document.querySelectorAll('[data-zapret-pane]').forEach((tab) => {
    tab.addEventListener('click', () => showZapretSettingsPane(tab.dataset.zapretPane));
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
