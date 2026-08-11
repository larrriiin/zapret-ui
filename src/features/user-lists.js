import { $, invoke } from '../lib/core.js';
import { escapeHtml, cleanAndValidateDomain, validateIP, showConfirm } from '../lib/dom.js';
import { markRestartIfServiceRunning } from '../lib/restart.js';
import { t } from '../lib/i18n.js';

const listsCache = {
  'list-general-user.txt': [],
  'list-exclude-user.txt': [],
  'ipset-exclude-user.txt': []
};

export function filterAndRender(containerId, filename, searchInputId) {
  const searchInput = $(searchInputId);
  const query = searchInput ? searchInput.value.toLowerCase().trim() : '';
  const items = listsCache[filename] || [];
  const filtered = query ? items.filter(item => item.toLowerCase().includes(query)) : items;
  renderList(containerId, filtered, filename);
}

export async function loadUserLists() {
  try {
    listsCache['list-general-user.txt'] = await invoke('read_user_list', { filename: 'list-general-user.txt' });
    filterAndRender('site-include-list', 'list-general-user.txt', 'site-include-search');

    listsCache['list-exclude-user.txt'] = await invoke('read_user_list', { filename: 'list-exclude-user.txt' });
    filterAndRender('site-exclude-list', 'list-exclude-user.txt', 'site-exclude-search');

    listsCache['ipset-exclude-user.txt'] = await invoke('read_user_list', { filename: 'ipset-exclude-user.txt' });
    filterAndRender('ip-exclude-list', 'ipset-exclude-user.txt', 'ip-exclude-search');
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
    input.classList.add('border-error-dim', 'animate-shake');
    setTimeout(() => input.classList.remove('border-error-dim', 'animate-shake'), 2000);
    return;
  }

  // Check for duplicates
  const existingItems = listsCache[filename] || [];
  if (existingItems.includes(validatedValue)) {
    input.classList.add('border-error-dim', 'animate-shake');
    const originalPlaceholder = input.placeholder;
    input.value = '';
    input.placeholder = t('duplicate_warning');
    setTimeout(() => {
      input.classList.remove('border-error-dim', 'animate-shake');
      input.placeholder = originalPlaceholder;
    }, 2000);
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
  const pasteLabel = $('list-import-paste-label');
  const isIPList = filename.includes('ipset');
  
  if (titleEl) titleEl.textContent = title;
  if (pasteLabel) pasteLabel.textContent = t(isIPList ? 'import_paste_ips' : 'import_paste_domains');
  if (textarea) {
    textarea.value = '';
    textarea.placeholder = isIPList
      ? '192.168.1.1\n10.0.0.0/24'
      : 'example.com\ngoogle.com\nyoutube.com';
  }
  
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
  $('ip-exclude-export-btn')?.addEventListener('click', () => handleExport('ipset-exclude-user.txt', 'ip-exclude-export-btn'));

  // Bind Bulk Add/Import buttons
  $('site-include-bulk-btn')?.addEventListener('click', () => openImportModal('list-general-user.txt', t('import_title_include')));
  $('site-exclude-bulk-btn')?.addEventListener('click', () => openImportModal('list-exclude-user.txt', t('import_title_exclude')));
  $('ip-exclude-bulk-btn')?.addEventListener('click', () => openImportModal('ipset-exclude-user.txt', t('import_title_ip')));

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

  // Bind Search events
  const setupSearch = (inputId, containerId, filename) => {
    const input = $(inputId);
    if (input) {
      input.addEventListener('input', () => {
        filterAndRender(containerId, filename, inputId);
      });
    }
  };
  setupSearch('site-include-search', 'site-include-list', 'list-general-user.txt');
  setupSearch('site-exclude-search', 'site-exclude-list', 'list-exclude-user.txt');
  setupSearch('ip-exclude-search', 'ip-exclude-list', 'ipset-exclude-user.txt');

  // Bind Clear events
  const setupClear = (btnId, filename) => {
    const btn = $(btnId);
    if (btn) {
      btn.onclick = async () => {
        const confirmed = await showConfirm(t('confirm_clear'));
        if (!confirmed) return;
        try {
          await invoke('write_user_list', { filename, lines: [] });
          await loadUserLists();
          await markRestartIfServiceRunning();
        } catch (err) {
          console.error(`Error clearing list ${filename}:`, err);
        }
      };
    }
  };
  setupClear('site-include-clear-btn', 'list-general-user.txt');
  setupClear('site-exclude-clear-btn', 'list-exclude-user.txt');
  setupClear('ip-exclude-clear-btn', 'ipset-exclude-user.txt');

  // Bind Backup events
  for (const exportBtn of [$('backup-export-btn'), $('ip-backup-export-btn')].filter(Boolean)) {
    exportBtn.onclick = async () => {
      try {
        const exported = await invoke('export_backup_file');
        if (exported) {
          const origHTML = exportBtn.innerHTML;
          exportBtn.innerHTML = `
            <span class="material-symbols-outlined text-sm">check</span>
            <span data-i18n="done">${t('done')}</span>
          `;
          exportBtn.classList.add('text-secondary', 'border-secondary/40');
          setTimeout(() => {
            exportBtn.innerHTML = origHTML;
            exportBtn.classList.remove('text-secondary', 'border-secondary/40');
          }, 2000);
        }
      } catch (err) {
        console.error('Failed to export backup:', err);
      }
    };
  }

  for (const importBtn of [$('backup-import-btn'), $('ip-backup-import-btn')].filter(Boolean)) {
    importBtn.onclick = async () => {
      const confirmed = await showConfirm(t('confirm_backup_restore'));
      if (!confirmed) return;
      try {
        const imported = await invoke('import_backup_file');
        if (imported) {
          await loadUserLists();
          await markRestartIfServiceRunning();
          const origHTML = importBtn.innerHTML;
          importBtn.innerHTML = `
            <span class="material-symbols-outlined text-sm">check</span>
            <span data-i18n="done">${t('done')}</span>
          `;
          importBtn.classList.add('text-secondary', 'border-secondary/40');
          setTimeout(() => {
            importBtn.innerHTML = origHTML;
            importBtn.classList.remove('text-secondary', 'border-secondary/40');
          }, 2000);
        }
      } catch (err) {
        console.error('Failed to import backup:', err);
      }
    };
  }

  // Bind Search Toggles
  const bindSearchToggle = (btnId, containerId, inputId) => {
    const btn = $(btnId);
    const container = $(containerId);
    const input = $(inputId);
    if (btn && container && input) {
      btn.onclick = () => {
        const isHidden = container.classList.toggle('hidden');
        if (!isHidden) {
          input.focus();
        } else {
          input.value = '';
          input.dispatchEvent(new Event('input'));
        }
      };
    }
  };
  bindSearchToggle('site-include-search-toggle', 'site-include-search-container', 'site-include-search');
  bindSearchToggle('site-exclude-search-toggle', 'site-exclude-search-container', 'site-exclude-search');
  bindSearchToggle('ip-exclude-search-toggle', 'ip-exclude-search-container', 'ip-exclude-search');
}
