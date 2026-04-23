import { $, invoke } from '../lib/core.js';
import { escapeHtml, cleanAndValidateDomain, validateIP } from '../lib/dom.js';
import { markRestartIfServiceRunning } from '../lib/restart.js';

export async function loadUserLists() {
  try {
    const includeList = await invoke('read_user_list', { filename: 'list-general-user.txt' });
    renderList('site-include-list', includeList, 'list-general-user.txt');

    const excludeList = await invoke('read_user_list', { filename: 'list-exclude-user.txt' });
    renderList('site-exclude-list', excludeList, 'list-exclude-user.txt');

    const ipExcludeList = await invoke('read_user_list', { filename: 'ipset-exclude-user.txt' });
    renderList('ip-exclude-list', ipExcludeList, 'ipset-exclude-user.txt');
  } catch (err) {
    console.error('Error loading user lists:', err);
  }
}

function renderList(containerId, items, filename) {
  const container = $(containerId);
  if (!container) return;
  container.innerHTML = '';

  items.forEach((item) => {
    const row = document.createElement('div');
    row.className = 'flex items-center justify-between bg-surface-container-highest/50 rounded-xl px-4 py-2';
    row.innerHTML = `
      <span class="text-sm text-on-surface truncate">${escapeHtml(item)}</span>
      <button class="delete-btn text-error-dim hover:text-error-dim/70 transition-colors" data-item="${escapeHtml(item)}">
        <span class="material-symbols-outlined text-lg">delete</span>
      </button>
    `;

    row.querySelector('.delete-btn').addEventListener('click', async () => {
      try {
        await invoke('remove_from_user_list', { filename, entry: item });
        await loadUserLists();
        await markRestartIfServiceRunning();
      } catch (err) {
        console.error('Error removing item:', err);
      }
    });

    container.appendChild(row);
  });
}

export async function addToList(inputId, filename) {
  const input = $(inputId);
  if (!input) return;
  const value = input.value.trim();
  if (!value) return;

  let validatedValue;
  if (filename.includes('ipset')) {
    validatedValue = validateIP(value);
  } else {
    validatedValue = cleanAndValidateDomain(value);
  }

  if (!validatedValue) {
    input.classList.add('border-error-dim');
    setTimeout(() => input.classList.remove('border-error-dim'), 2000);
    return;
  }

  try {
    await invoke('add_to_user_list', { filename, entry: validatedValue });
    input.value = '';
    await loadUserLists();
    await markRestartIfServiceRunning();
  } catch (err) {
    console.error('Error adding item:', err);
  }
}

export function initUserLists() {
  const bindAddList = (btnId, inputId, filename) => {
    const btn = $(btnId);
    const input = $(inputId);
    if (btn) btn.onclick = () => addToList(inputId, filename);
    if (input) {
      input.onkeypress = (e) => {
        if (e.key === 'Enter') addToList(inputId, filename);
      };
    }
  };
  bindAddList('site-include-add', 'site-include-input', 'list-general-user.txt');
  bindAddList('site-exclude-add', 'site-exclude-input', 'list-exclude-user.txt');
  bindAddList('ip-exclude-add', 'ip-exclude-input', 'ipset-exclude-user.txt');
}
