import { $, invoke } from '../lib/core.js';
import { escapeHtml, cleanAndValidateDomain, validateIP } from '../lib/dom.js';
import { markRestartIfServiceRunning } from '../lib/restart.js';
import { t } from '../lib/i18n.js';

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

let currentImportFilename = '';

async function handleExport(filename, btnId) {
  const btn = $(btnId);
  if (!btn) return;

  try {
    const saved = await invoke('save_user_list_to_file', { filename });
    if (!saved) return; // User cancelled the save dialog

    const originalHTML = btn.innerHTML;
    btn.innerHTML = `
      <span class="material-symbols-outlined text-sm">check</span>
      <span data-i18n="done">${t('done')}</span>
    `;
    
    // Add success styling temporarily
    const isInclude = filename.includes('general');
    const accentClass = isInclude ? 'text-secondary' : 'text-error-dim';
    const borderClass = isInclude ? 'border-secondary/40' : 'border-error-dim/40';
    btn.classList.add(accentClass, borderClass);

    setTimeout(() => {
      btn.innerHTML = originalHTML;
      btn.classList.remove(accentClass, borderClass);
    }, 2000);
  } catch (err) {
    console.error('Error exporting list:', err);
  }
}

function openImportModal(filename, title) {
  currentImportFilename = filename;
  const modal = $('list-import-modal');
  const titleEl = $('list-import-title');
  const textarea = $('list-import-textarea');
  
  if (titleEl) titleEl.textContent = title;
  if (textarea) textarea.value = '';
  
  modal?.classList.remove('hidden');
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

  // Bind Export/Share buttons
  $('site-include-export-btn')?.addEventListener('click', () => handleExport('list-general-user.txt', 'site-include-export-btn'));
  $('site-exclude-export-btn')?.addEventListener('click', () => handleExport('list-exclude-user.txt', 'site-exclude-export-btn'));

  // Bind Bulk Add/Import buttons
  $('site-include-bulk-btn')?.addEventListener('click', () => openImportModal('list-general-user.txt', t('import_title_include')));
  $('site-exclude-bulk-btn')?.addEventListener('click', () => openImportModal('list-exclude-user.txt', t('import_title_exclude')));

  // Modal event bindings
  const modal = $('list-import-modal');
  const closeBtn = $('list-import-cancel');
  const closeX = $('list-import-close-x');
  const submitBtn = $('list-import-submit');
  const textarea = $('list-import-textarea');
  const fileInput = $('list-import-file-input');
  const fileBtn = $('list-import-file-btn');

  const closeModal = () => {
    modal?.classList.add('hidden');
    currentImportFilename = '';
    if (textarea) textarea.value = '';
    if (fileInput) fileInput.value = '';
  };

  closeBtn?.addEventListener('click', closeModal);
  closeX?.addEventListener('click', closeModal);
  modal?.addEventListener('click', (e) => {
    if (e.target === modal) closeModal();
  });

  if (fileBtn && fileInput) {
    fileBtn.onclick = () => fileInput.click();
    fileInput.onchange = async (e) => {
      const file = e.target.files[0];
      if (file) {
        try {
          const text = await file.text();
          if (textarea) {
            textarea.value = text;
          }
        } catch (err) {
          console.error('Failed to read file:', err);
        }
        fileInput.value = '';
      }
    };
  }

  submitBtn?.addEventListener('click', async () => {
    if (!currentImportFilename || !textarea) return;
    const text = textarea.value.trim();
    if (!text) {
      textarea.classList.add('border-error-dim');
      setTimeout(() => textarea.classList.remove('border-error-dim'), 2000);
      return;
    }

    const modeElements = document.getElementsByName('import-mode');
    let importMode = 'append';
    for (const el of modeElements) {
      if (el.checked) {
        importMode = el.value;
        break;
      }
    }

    const isReplace = importMode === 'replace';

    // Parse lines
    const rawLines = text.split(/\r?\n/);
    const validatedLines = [];

    for (let line of rawLines) {
      line = line.trim();
      if (!line) continue;
      
      let validatedValue;
      if (currentImportFilename.includes('ipset')) {
        validatedValue = validateIP(line);
      } else {
        validatedValue = cleanAndValidateDomain(line);
      }

      if (validatedValue && !validatedLines.includes(validatedValue)) {
        validatedLines.push(validatedValue);
      }
    }

    if (validatedLines.length === 0) {
      textarea.classList.add('border-error-dim');
      setTimeout(() => textarea.classList.remove('border-error-dim'), 2000);
      return;
    }

    try {
      let finalLines = [];
      if (isReplace) {
        finalLines = validatedLines;
      } else {
        // Append mode: load current list first
        const currentList = await invoke('read_user_list', { filename: currentImportFilename });
        const set = new Set([...currentList, ...validatedLines]);
        finalLines = Array.from(set);
      }

      await invoke('write_user_list', { filename: currentImportFilename, lines: finalLines });
      await loadUserLists();
      await markRestartIfServiceRunning();
      closeModal();
    } catch (err) {
      console.error('Failed to import:', err);
    }
  });
}
