import { $, invoke } from '../lib/core.js';
import { restartServiceIfRunning } from '../lib/restart.js';
import { t } from '../lib/i18n.js';

let availableFakes = [];
let currentDiscordFake = 'quic_initial_steamcommunity_com';
let currentGameFake = 'quic_initial_dbankcloud_ru';

export function closeFakeDropdowns() {
  const discordPanel = $('discord-fake-options');
  const discordChevron = $('discord-fake-chevron');
  if (discordPanel) discordPanel.classList.add('hidden');
  if (discordChevron) discordChevron.style.transform = '';

  const gamePanel = $('game-fake-options');
  const gameChevron = $('game-fake-chevron');
  if (gamePanel) gamePanel.classList.add('hidden');
  if (gameChevron) gameChevron.style.transform = '';
}

function renderFakeOptionsList(type) {
  const isDiscord = type === 'discord';
  const listContainer = $(isDiscord ? 'discord-fake-options-list' : 'game-fake-options-list');
  const activeFake = isDiscord ? currentDiscordFake : currentGameFake;

  if (!listContainer) return;
  listContainer.innerHTML = '';

  if (availableFakes.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'px-4 py-3 text-xs text-on-surface-variant/60 italic font-mono';
    empty.textContent = t('no_fake_files');
    listContainer.appendChild(empty);
    return;
  }

  availableFakes.forEach((fake) => {
    const isSelected = fake.name === activeFake;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = `w-full text-left px-4 py-2.5 text-xs font-mono transition-colors flex items-center justify-between cursor-pointer ${
      isSelected
        ? 'bg-primary/20 text-primary font-bold'
        : 'text-on-surface/80 hover:bg-primary/10 hover:text-on-surface'
    }`;

    btn.innerHTML = `
      <span class="truncate">${fake.name}</span>
      ${isSelected ? '<span class="material-symbols-outlined text-sm text-primary ml-2">check</span>' : ''}
    `;

    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      closeFakeDropdowns();
      handleFakeChange(type, fake.name);
    });

    listContainer.appendChild(btn);
  });
}

export function updateFakesUI(fakesInfo) {
  if (!fakesInfo) return;

  currentDiscordFake = fakesInfo.current_discord_fake || 'quic_initial_steamcommunity_com';
  currentGameFake = fakesInfo.current_game_fake || 'quic_initial_dbankcloud_ru';
  availableFakes = fakesInfo.available_fakes || [];

  const discordLabel = $('discord-fake-label');
  if (discordLabel) {
    discordLabel.textContent = currentDiscordFake;
  }

  const gameLabel = $('game-fake-label');
  if (gameLabel) {
    gameLabel.textContent = currentGameFake;
  }

  renderFakeOptionsList('discord');
  renderFakeOptionsList('game');
}

export async function pollFakes() {
  try {
    const fakesInfo = await invoke('get_fakes_info');
    updateFakesUI(fakesInfo);
  } catch (err) {
    console.error('Error polling fakes info:', err);
  }
}

export async function handleFakeChange(fakeType, fakeName) {
  if (!fakeName) return;
  try {
    if (fakeType === 'discord') currentDiscordFake = fakeName;
    if (fakeType === 'game') currentGameFake = fakeName;

    const discordLabel = $('discord-fake-label');
    if (fakeType === 'discord' && discordLabel) discordLabel.textContent = fakeName;
    const gameLabel = $('game-fake-label');
    if (fakeType === 'game' && gameLabel) gameLabel.textContent = fakeName;

    await invoke('set_active_fake', { fakeType, fakeName });
    await pollFakes();
    await restartServiceIfRunning();
  } catch (err) {
    console.error(`Error replacing ${fakeType} fake:`, err);
  }
}

export function initFakeSelectors() {
  // Discord Dropdown
  const discordTrigger = $('discord-fake-trigger');
  const discordPanel = $('discord-fake-options');
  const discordChevron = $('discord-fake-chevron');

  // Append panels to body to escape overflow contexts and backdrop-filters
  const gameTrigger = $('game-fake-trigger');
  const gamePanel = $('game-fake-options');
  const gameChevron = $('game-fake-chevron');

  if (discordPanel) document.body.appendChild(discordPanel);
  if (gamePanel) document.body.appendChild(gamePanel);

  if (discordTrigger && discordPanel) {
    discordTrigger.addEventListener('click', (e) => {
      e.stopPropagation();
      const willOpen = discordPanel.classList.contains('hidden');
      closeFakeDropdowns();
      if (willOpen) {
        discordPanel.classList.remove('hidden');
        if (discordChevron) discordChevron.style.transform = 'rotate(180deg)';
        renderFakeOptionsList('discord');
        
        // Position relative to viewport since it's on body
        const rect = discordTrigger.getBoundingClientRect();
        discordPanel.style.position = 'fixed';
        discordPanel.style.left = rect.left + 'px';
        discordPanel.style.width = rect.width + 'px';
        discordPanel.style.marginTop = '0';
        
        // Drop-up if not enough space below (assuming max height is ~240px)
        if (rect.bottom + 240 > window.innerHeight && rect.top > 240) {
          discordPanel.style.top = 'auto';
          discordPanel.style.bottom = (window.innerHeight - rect.top + 4) + 'px';
        } else {
          discordPanel.style.bottom = 'auto';
          discordPanel.style.top = (rect.bottom + 4) + 'px';
        }
      }
    });
  }

  if (gameTrigger && gamePanel) {
    gameTrigger.addEventListener('click', (e) => {
      e.stopPropagation();
      const willOpen = gamePanel.classList.contains('hidden');
      closeFakeDropdowns();
      if (willOpen) {
        gamePanel.classList.remove('hidden');
        if (gameChevron) gameChevron.style.transform = 'rotate(180deg)';
        renderFakeOptionsList('game');
        
        // Position relative to viewport since it's on body
        const rect = gameTrigger.getBoundingClientRect();
        gamePanel.style.position = 'fixed';
        gamePanel.style.left = rect.left + 'px';
        gamePanel.style.width = rect.width + 'px';
        gamePanel.style.marginTop = '0';
        
        // Drop-up if not enough space below (assuming max height is ~240px)
        if (rect.bottom + 240 > window.innerHeight && rect.top > 240) {
          gamePanel.style.top = 'auto';
          gamePanel.style.bottom = (window.innerHeight - rect.top + 4) + 'px';
        } else {
          gamePanel.style.bottom = 'auto';
          gamePanel.style.top = (rect.bottom + 4) + 'px';
        }
      }
    });
  }

  // Handle scrolling of main container to close dropdowns or update positions
  const mainContent = document.getElementById('main-content') || document.querySelector('main');
  if (mainContent) {
    mainContent.addEventListener('scroll', closeFakeDropdowns, { passive: true });
  }

  // Close on outside click
  document.addEventListener('click', (e) => {
    if (!$('discord-fake-dropdown')?.contains(e.target) && !$('game-fake-dropdown')?.contains(e.target)) {
      closeFakeDropdowns();
    }
  });
}
