import { $ } from '../lib/core.js';
import { t } from '../lib/i18n.js';

export function renderRuleList(list, values, { kind, disabled, remove, details }) {
  const application = kind === 'apps';
  const removeLabel = t(application ? 'warp_apps_remove' : 'warp_sites_remove');
  const pathLabel = application ? t('warp_apps_path') : '';
  const signature = JSON.stringify([values, disabled, removeLabel, pathLabel]);
  if (list.dataset.signature === signature) return;
  list.dataset.signature = signature;
  list.replaceChildren(...values.map(value => {
    const row = document.createElement('li');
    row.className = 'warp-rule-card';
    const icon = document.createElement('span');
    icon.className = 'material-symbols-outlined';
    icon.setAttribute('aria-hidden', 'true');
    icon.textContent = application ? 'description' : 'language';
    const name = document.createElement('span');
    name.className = 'warp-rule-name';
    name.textContent = application ? value.split(/[\\/]/).pop() || value : value;
    name.title = value;
    const actions = document.createElement('div');
    actions.className = 'warp-card-actions';
    if (details) actions.append(iconButton('info', pathLabel, name.textContent, () => details(value)));
    const button = iconButton('close', removeLabel, name.textContent, () => remove(value));
    button.disabled = disabled;
    actions.append(button);
    row.append(icon, name, actions);
    return row;
  }));
}

function iconButton(icon, label, name, run) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'ui-icon-button';
  button.title = label;
  button.setAttribute('aria-label', `${label}: ${name}`);
  button.innerHTML = `<span class="material-symbols-outlined" aria-hidden="true">${icon}</span>`;
  button.addEventListener('click', run);
  return button;
}

function normalizeSiteDomain(value) {
  const candidate = value.trim();
  if (!candidate) return '';
  try {
    const source = /^[a-z][a-z\d+.-]*:\/\//i.test(candidate)
      ? candidate
      : `https://${candidate.replace(/^\/+/, '')}`;
    return new URL(source).hostname.replace(/\.$/, '').toLowerCase();
  } catch {
    return candidate;
  }
}

export function initSiteEntryDialog() {
  const dialog = $('warp-sites-dialog');
  const input = $('warp-sites-input');
  let accept;
  document.body.append(dialog);
  $('warp-sites-dialog-close').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => { if (event.target === dialog) dialog.close(); });
  $('warp-sites-dialog-add').addEventListener('click', () => {
    accept([...new Set(input.value.split(/\r?\n/).map(normalizeSiteDomain).filter(Boolean))]);
    dialog.close();
  });
  return onAdd => {
    accept = onAdd;
    input.value = '';
    dialog.showModal();
    input.focus();
  };
}
