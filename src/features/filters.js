import { $, invoke } from '../lib/core.js';
import { state } from '../lib/state.js';
import { restartServiceIfRunning } from '../lib/restart.js';
import { t } from '../lib/i18n.js';

function setCardActive(id, active) {
  const el = $(id);
  if (!el) return;
  if (active) el.classList.add('card-active');
  else el.classList.remove('card-active');
}

function setToggle(id, on) {
  const btn = $(id);
  if (!btn) return;
  if (on) {
    btn.classList.remove('is-off');
    btn.classList.add('is-on');
  } else {
    btn.classList.remove('is-on');
    btn.classList.add('is-off');
  }
}

function setCardDisabled(id, disabled) {
  const el = $(id);
  if (!el) return;
  if (disabled) {
    el.classList.add('card-disabled');
    el.style.pointerEvents = 'none';
    el.style.opacity = '0.4';
  } else {
    el.classList.remove('card-disabled');
    el.style.pointerEvents = 'auto';
    el.style.opacity = '1';
  }
}

export function updateFiltersUI(filters) {
  const ipsetOn = filters.ipset !== 'none';
  setToggle('ipset-toggle', ipsetOn);
  setCardActive('ipset-loaded', filters.ipset === 'loaded');
  setCardActive('ipset-any', filters.ipset === 'any');
  setCardDisabled('ipset-loaded', !ipsetOn);
  setCardDisabled('ipset-any', !ipsetOn);

  const gameOn = filters.game_filter !== 'disabled';
  setToggle('game-toggle', gameOn);
  setCardActive('game-all', filters.game_filter === 'all');
  setCardActive('game-tcp', filters.game_filter === 'tcp');
  setCardActive('game-udp', filters.game_filter === 'udp');
  setCardDisabled('game-all', !gameOn);
  setCardDisabled('game-tcp', !gameOn);
  setCardDisabled('game-udp', !gameOn);

  const tcpRange = $('game-tcp-range');
  const udpRange = $('game-udp-range');
  if (tcpRange && document.activeElement !== tcpRange) tcpRange.value = filters.game_tcp_range || '1024-65535';
  if (udpRange && document.activeElement !== udpRange) udpRange.value = filters.game_udp_range || '1024-65535';
}

export async function pollFilters() {
  try {
    const filters = await invoke('get_filters_status');
    state.currentFilters = filters;
    if (filters.game_filter !== 'disabled') {
      state.previousGameFilter = filters.game_filter;
    }
    if (filters.ipset !== 'none') {
      state.previousIPSet = filters.ipset;
    }
    updateFiltersUI(filters);
  } catch (err) {
    console.error('Ошибка опроса фильтров:', err);
  }
}

export async function handleGameFilterChange(mode) {
  try {
    await invoke('set_game_filter', { mode });
    await pollFilters();
    await restartServiceIfRunning();
  } catch (err) {
    console.error('Ошибка смены Game Filter:', err);
  }
}

export async function handleIPSetFilterChange(mode) {
  try {
    await invoke('set_ipset_filter', { mode });
    await pollFilters();
    await restartServiceIfRunning();
  } catch (err) {
    console.error('Ошибка смены IPSet Filter:', err);
  }
}

export function initFilterButtons() {
  const portsModal = $('game-ports-modal');
  const portsOpen = $('game-ports-open');
  const portsStatus = $('game-ports-status');
  let lastFocus = null;
  const closePorts = () => {
    portsModal?.classList.add('hidden');
    portsStatus?.classList.add('hidden');
    lastFocus?.focus({ preventScroll: true });
  };
  const openPorts = () => {
    lastFocus = portsOpen;
    const filters = state.currentFilters;
    const tcpRange = $('game-tcp-range');
    const udpRange = $('game-udp-range');
    if (tcpRange) tcpRange.value = filters?.game_tcp_range || '1024-65535';
    if (udpRange) udpRange.value = filters?.game_udp_range || '1024-65535';
    portsStatus?.classList.add('hidden');
    portsModal?.classList.remove('hidden');
    tcpRange?.focus({ preventScroll: true });
  };
  portsOpen?.addEventListener('click', openPorts);
  $('game-ports-close')?.addEventListener('click', closePorts);
  $('game-ports-cancel')?.addEventListener('click', closePorts);
  portsModal?.addEventListener('click', (event) => {
    if (event.target === portsModal) closePorts();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !portsModal?.classList.contains('hidden')) {
      event.preventDefault();
      closePorts();
    }
  });

  $('game-toggle')?.addEventListener('click', () => {
    const isOn = state.currentFilters.game_filter !== 'disabled';
    if (isOn) {
      state.previousGameFilter = state.currentFilters.game_filter;
      handleGameFilterChange('disabled');
    } else {
      handleGameFilterChange(state.previousGameFilter);
    }
  });
  $('game-all')?.addEventListener('click', () => handleGameFilterChange('all'));
  $('game-tcp')?.addEventListener('click', () => handleGameFilterChange('tcp'));
  $('game-udp')?.addEventListener('click', () => handleGameFilterChange('udp'));

  $('game-port-settings')?.addEventListener('submit', async (event) => {
    event.preventDefault();
    const tcpRange = $('game-tcp-range')?.value.trim();
    const udpRange = $('game-udp-range')?.value.trim();
    const mode = state.currentFilters?.game_filter || 'disabled';
    try {
      await invoke('set_game_filter', { mode, tcpRange, udpRange });
      portsStatus?.classList.add('hidden');
      await pollFilters();
      await restartServiceIfRunning();
      closePorts();
    } catch (err) {
      if (portsStatus) {
        portsStatus.textContent = String(err) === 'invalid_game_filter_range'
          ? t('game_ports_invalid')
          : t('game_ports_save_failed');
        portsStatus.classList.remove('hidden');
      }
    }
  });

  $('ipset-toggle')?.addEventListener('click', () => {
    const isOn = state.currentFilters.ipset !== 'none';
    if (isOn) {
      state.previousIPSet = state.currentFilters.ipset;
      handleIPSetFilterChange('none');
    } else {
      handleIPSetFilterChange(state.previousIPSet);
    }
  });
  $('ipset-loaded')?.addEventListener('click', () => handleIPSetFilterChange('loaded'));
  $('ipset-any')?.addEventListener('click', () => handleIPSetFilterChange('any'));
}
