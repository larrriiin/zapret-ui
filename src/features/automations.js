import './automations.css';

import { createRuleSorter, createRuleMenu } from './automations-interactions.js';

import { $, invoke } from '../lib/core.js';

import { t, onLangChange } from '../lib/i18n.js';

import { showConfirm } from '../lib/dom.js';

import { createWarpDropdown } from './warp-mode.js';



let config = null, status = {}, editing = null, busy = false, fetching = false;

let strategies = [], modes = [];

let selectedIcon = 'apps', sorter, menu;

let routeCatalog = null, useSavedRoutes = true, loadingRoutes = false, editorRequest = 0;

let selectedRoutes = { sites: new Set(), applications: new Set() };

function renderProxyPicker() {

  const visible = $('auto-mode').value === 'proxy';

  $('auto-proxy-picker').hidden = !visible;

  $('auto-proxy-custom').hidden = useSavedRoutes;

  document.querySelectorAll('[name="auto-route-source"]').forEach(input => { input.checked = (input.value === 'settings') === useSavedRoutes; input.disabled = busy; });

  $('auto-proxy-retry').hidden = !!routeCatalog || loadingRoutes;

  for (const kind of ['sites', 'applications']) {

    const list = $(`auto-proxy-${kind}`), known = routeCatalog?.[kind] || [];

    const options = [...new Set([...known, ...selectedRoutes[kind]])];

    list.replaceChildren(...options.map(value => {

      const row = document.createElement('label'); row.className = 'ui-list-row text-sm';

      const input = document.createElement('input'); input.type = 'checkbox'; input.className = 'ui-check'; input.value = value;

      input.checked = selectedRoutes[kind].has(value); input.disabled = busy || loadingRoutes || (!known.includes(value) && !input.checked);

      input.addEventListener('change', () => { if (input.checked) selectedRoutes[kind].add(value); else selectedRoutes[kind].delete(value); if (!known.includes(value)) input.disabled = true; });

      const text = document.createElement('span'); text.textContent = (kind === 'sites' ? value : value.split(/[\\/]/).at(-1)) + (!known.includes(value) ? ` · ${t('auto_proxy_removed')}` : ''); text.title = value;

      row.append(input, text); return row;

    }));

    if (!options.length) { const empty = document.createElement('p'); empty.className = 'text-xs text-on-surface-variant'; empty.textContent = t(loadingRoutes ? 'auto_loading' : routeCatalog ? 'auto_proxy_empty' : 'auto_proxy_load_error'); list.append(empty); }

  }

}

async function loadEditorChoices(request = editorRequest) {

  loadingRoutes = true; renderProxyPicker();

  const results = await Promise.allSettled([invoke('get_strategies'), invoke('get_warp_status')]);

  if (request !== editorRequest) return;

  loadingRoutes = false;

  if (results[0].status === 'fulfilled') strategies = results[0].value;

  if (results[1].status === 'fulfilled' && !results[1].value.error) {

    const warp = results[1].value;

    modes = warp.modes || [];

    routeCatalog = { sites: warp.sites?.domains || [], applications: warp.applications?.paths || [] };

  } else { routeCatalog = null; }

  renderOptions();

}

const icons = ['apps', 'sports_esports', 'forum', 'public', 'music_note', 'movie', 'work', 'code', 'school', 'cloud', 'shield', 'bolt'];

function renderIcons() {

  $('auto-icons').replaceChildren(...icons.map(icon => {

    const button = iconButton(icon, `auto_icon_${icon}`, () => { selectedIcon = icon; renderIcons(); $('auto-icons').querySelector(`[data-icon="${icon}"]`)?.focus(); });

    button.className = 'ui-tab'; button.dataset.icon = icon;

    button.setAttribute('aria-pressed', String(selectedIcon === icon)); button.disabled = busy; return button;

  }));

}

function focusHandle(id) {

  [...$('auto-list').children].find(c => c.dataset.ruleId === id)?.querySelector('.auto-drag-handle')?.focus({ preventScroll: true });

}

async function saveOrder(order, id) {

  const ok = await mutate(c => { const byId = new Map(c.rules.map(r => [r.id, r])); c.rules = order.map(id => byId.get(id)); });

  if (!ok) render();

  focusHandle(id);

  if (ok) $('auto-announcement').textContent = t('auto_moved', { name: config.rules.find(r => r.id === id)?.name, position: order.indexOf(id) + 1 });

}

const fields = ['zapret', 'strategy', 'warp', 'mode', 'close'];

const dropdowns = Object.fromEntries(fields.map(name => [name, createWarpDropdown(`auto-${name}`)]));

const errorText = error => typeof error === 'string' ? error : error?.detail || error?.message || String(error);

function showError(error) {
  const modal = $('auto-editor').open;
  for (const id of ['auto-error', 'auto-editor-error']) {
    const target = $(id), selected = id === (modal ? 'auto-editor-error' : 'auto-error');
    target.hidden = !error || !selected;
    target.textContent = error && selected ? t('auto_error') + errorText(error) : '';
    if (error && selected && modal) target.scrollIntoView({ block:'nearest' });
  }
}

function controls() {

  $('auto-add').disabled = busy || !config;

  $('auto-enabled').disabled = busy || !config;

  $('auto-save').disabled = busy;

  $('auto-choose').disabled = busy;
  $('auto-cancel').disabled = busy;
  $('auto-editor-close').disabled = busy;
  $('auto-name').disabled = busy;

  document.querySelectorAll('#auto-icons button').forEach(button => { button.disabled = busy; });

  $('auto-enabled').dataset.enabled = String(!!config?.enabled);

  $('auto-enabled').setAttribute('aria-checked', String(!!config?.enabled));

  document.querySelectorAll('#auto-list button').forEach(button => { button.disabled = busy; });

}

function renderOptions() {

  const keep = ['keep', t('auto_keep')];

  const options = {

    zapret: [keep, ['on', t('auto_on')], ['off', t('auto_off')]],

    warp: [keep, ['on', t('auto_connect')], ['off', t('auto_disconnect')]],

    strategy: [keep, ...strategies.map(s => [s, s])],

    mode: [keep, ...modes.map(m => [m, t(`warp_mode_${m.replaceAll("+", "_")}`) === `warp_mode_${m.replaceAll("+", "_")}` ? m : t(`warp_mode_${m.replaceAll("+", "_")}`)])],

    close: [['restore', t('auto_restore')], ['leave', t('auto_leave')], ['off', t('auto_close_off')]],

  };

  for (const field of fields) {

    const select = $(`auto-${field}`), value = select.value || (field === 'close' ? 'restore' : 'keep');

    if (!options[field].some(([v]) => v === value)) options[field].push([value, value]);

    select.replaceChildren(...options[field].map(([v, text]) => new Option(text, v)));

    select.value = value;

    dropdowns[field].render(options[field], value, busy);

  }

  renderProxyPicker();

}

function iconButton(icon, key, action) {

  const button = document.createElement('button');

  button.type = 'button'; button.className = 'ui-icon-button'; button.title = t(key);

  button.setAttribute('aria-label', t(key));

  const symbol = document.createElement('span'); symbol.className = 'material-symbols-outlined';

  symbol.setAttribute('aria-hidden', 'true'); symbol.textContent = icon; button.append(symbol);

  button.addEventListener('click', action); return button;

}

function render() {

  $('auto-status').textContent = status.paused ? t('auto_paused') : '';

  $('auto-empty').hidden = !config || config.rules.length > 0;

  if (!config) return;

  menu?.close();

  $('auto-list').replaceChildren(...config.rules.map((rule, index) => {

    const card = document.createElement('article'); card.className = 'ui-panel auto-rule'; card.dataset.ruleId = rule.id;

    const handle = iconButton('drag_indicator', 'auto_reorder', () => {}); handle.classList.add('auto-drag-handle');

    handle.setAttribute('aria-label', `${t('auto_reorder')}: ${rule.name}`); handle.setAttribute('aria-describedby', 'auto-sort-hint');

    sorter.bind(handle, card);

    const icon = document.createElement('span'); icon.className = 'ui-icon-tile'; icon.setAttribute('aria-hidden', 'true');

    const symbol = document.createElement('span'); symbol.className = 'material-symbols-outlined'; symbol.textContent = icons.includes(rule.icon) ? rule.icon : 'apps'; icon.append(symbol);

    const copy = document.createElement('div'); copy.className = 'auto-rule-copy';

    const title = document.createElement('h3'); title.className = 'font-headline font-bold'; title.textContent = rule.name;

    const path = document.createElement('p'); path.className = 'text-xs text-on-surface-variant mt-1'; path.textContent = rule.path.split(/[\\/]/).at(-1); path.title = rule.path;

    const description = document.createElement('div'); description.className = 'auto-rule-description';
    const providers = document.createElement('div'); providers.className = 'auto-rule-providers';
    function addProvider(name, iconName, iconClass, action, choice, warp) {
      if (action === 'keep' && !choice) return;
      const provider = document.createElement('div'); provider.className = 'auto-rule-provider'; provider.dataset.provider = warp ? 'warp' : 'zapret';
      const glyph = document.createElement('span'); glyph.className = `material-symbols-outlined ${iconClass}`; glyph.textContent = iconName; glyph.setAttribute('aria-hidden', 'true');
      const content = document.createElement('div'); content.className = 'auto-rule-provider-copy';
      const label = document.createElement('span'); label.className = 'auto-rule-provider-name'; label.textContent = name;
      const value = document.createElement('span'); value.className = 'auto-rule-provider-value';
      const actionText = action === 'on' ? t(warp ? 'auto_connect' : 'auto_on') : action === 'off' ? t(warp ? 'auto_disconnect' : 'auto_off') : '';
      const choiceText = warp && choice ? t(`warp_mode_${choice.replaceAll('+', '_')}`) : choice;
      value.textContent = [actionText, choiceText].filter(Boolean).join(' · ');
      provider.title = `${name}: ${value.textContent}`;
      if (warp && rule.warp_routes) {
        const selection = t('auto_proxy_count', { sites: rule.warp_routes.sites.length, apps: rule.warp_routes.applications.length });
        provider.title += `\n${selection}`;
        const details = document.createElement('span'); details.className = 'sr-only'; details.textContent = selection; content.append(details);
      }
      content.prepend(label, value); provider.append(glyph, content); providers.append(provider);
    }
    addProvider('ZAPRET', 'tune', 'zapret-provider-icon', rule.zapret, rule.strategy, false);
    addProvider('WARP', 'cloud', 'warp-cloud', rule.warp, rule.warp_mode, true);
    description.append(providers);
    const active = !!(config.enabled && rule.enabled && status.active?.includes(rule.id));
    card.classList.toggle('ui-panel-active', active);
    const state = document.createElement('span'); state.className = 'sr-only'; state.dataset.ruleState = rule.id;
    state.hidden = !active; state.textContent = t('auto_active_short');
    description.append(state);
    copy.append(title, path);
    const closeText = `${t('auto_close')}: ${t(rule.close_off ? 'auto_close_off' : rule.restore ? 'auto_restore' : 'auto_leave')}`;
    const close = document.createElement('span'); close.className = 'material-symbols-outlined auto-rule-close'; close.textContent = rule.close_off ? 'power_settings_new' : rule.restore ? 'restore' : 'keep';
    close.title = closeText; close.role = 'img'; close.setAttribute('aria-label', closeText);

    const actions = document.createElement('div'); actions.className = 'auto-rule-actions';

    const toggle = document.createElement('button'); toggle.type = 'button'; toggle.className = 'ui-switch';

    toggle.role = 'switch'; toggle.dataset.enabled = String(rule.enabled); toggle.setAttribute('aria-checked', String(rule.enabled));

    toggle.setAttribute('aria-label', `${t('auto_toggle')}: ${rule.name}`);

    toggle.addEventListener('click', () => mutate(c => { c.rules[index].enabled = !rule.enabled; }));

    const more = iconButton('more_horiz', 'auto_menu', () => menu.open(more, [

      { key: 'auto_edit', icon: 'edit', run: () => openEditor(rule) },

      { key: 'auto_remove', icon: 'delete', danger: true, run: async () => {

        if (await showConfirm(t('auto_delete'))) {

          if (await mutate(c => { c.rules = c.rules.filter(r => r.id !== rule.id); })) $('auto-add').focus();

        }

      } },

    ]));

    more.classList.add('auto-more'); more.setAttribute('aria-haspopup', 'menu'); more.setAttribute('aria-expanded', 'false');

    more.setAttribute('aria-controls', 'auto-rule-menu'); more.setAttribute('aria-label', `${t('auto_menu')}: ${rule.name}`);

    actions.append(close, toggle, more);

    card.append(handle, icon, copy, description, actions); return card;

  }));

  controls();

}

async function refresh() {

  if (fetching || busy || sorter?.active || menu?.active || $('auto-editor').open) return;

  fetching = true;

  try {

    const next = await invoke('get_application_rules');

    const changed = JSON.stringify([config, status.active, status.paused]) !== JSON.stringify([next.config, next.active, next.paused]);

    config = next.config; status = next;

    $('auto-retry').hidden = true;

    showError(next.error);

    if (changed) render();

    controls();

  } catch (error) { showError(error); $('auto-retry').hidden = false; }

  finally { fetching = false; }

}

async function mutate(change) {

  if (busy || !config) return false;

  busy = true; menu?.close(); controls(); renderOptions(); showError(null);

  const next = structuredClone(config); change(next);

  try { await invoke('save_application_rules', { config: next }); config = next; render(); return true; }

  catch (error) { showError(error); return false; }

  finally { busy = false; controls(); renderOptions(); }

}

async function openEditor(rule = null) {

  const request = ++editorRequest;

  editing = rule?.id || null;

  useSavedRoutes = !rule?.warp_routes;

  routeCatalog = null;

  selectedRoutes = { sites: new Set(rule?.warp_routes?.sites || []), applications: new Set(rule?.warp_routes?.applications || []) };

  selectedIcon = icons.includes(rule?.icon) ? rule.icon : 'apps'; renderIcons();

  $('auto-name').value = rule?.name || '';

  $('auto-path').value = rule?.path || '';

  for (const [key, value] of Object.entries({ zapret: rule?.zapret, warp: rule?.warp, strategy: rule?.strategy, mode: rule?.warp_mode, close: rule?.close_off ? 'off' : rule?.restore === false ? 'leave' : 'restore' })) {

    const select = $(`auto-${key}`); select.replaceChildren(new Option(value || 'keep', value || 'keep'));

  }

  showError(null);
  $('auto-editor-title').textContent = t(editing ? 'auto_edit_rule' : 'auto_add');
  renderOptions();
  if (!$('auto-editor').open) $('auto-editor').showModal();

  $('auto-editor').scrollTop = 0; $('auto-name').focus();

  await loadEditorChoices(request);

}

export function initAutomations() {

  const info = $('auto-info-dialog');

  $('auto-info').addEventListener('click', () => { if (!info.open) info.showModal(); });

  $('auto-info-close').addEventListener('click', () => info.close());

  info.addEventListener('click', event => {

    const rect = info.getBoundingClientRect();

    if (event.target === info && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) info.close();

  });

  menu = createRuleMenu();

  sorter = createRuleSorter({ canStart: () => !busy && !fetching, save: saveOrder, restore: id => { render(); if (id) focusHandle(id); } });

  renderIcons();

  $('auto-proxy-retry').addEventListener('click', () => loadEditorChoices());

  document.querySelectorAll('[name="auto-route-source"]').forEach(input => input.addEventListener('change', () => { useSavedRoutes = input.value === 'settings'; renderProxyPicker(); }));

  Object.values(dropdowns).forEach(d => d.init($('auto-editor')));

  fields.forEach(field => $(`auto-${field}`).addEventListener('change', renderOptions));

  $('auto-add').addEventListener('click', () => openEditor());

  $('auto-retry').addEventListener('click', refresh);

  const editor = $('auto-editor');
  const closeEditor = () => { if (!busy) editor.close(); };
  $('auto-cancel').addEventListener('click', closeEditor);
  $('auto-editor-close').addEventListener('click', closeEditor);
  editor.addEventListener('cancel', event => { if (busy) event.preventDefault(); });
  editor.addEventListener('close', () => {
    ++editorRequest;
    Object.values(dropdowns).forEach(d => d.close());
    const trigger = [...$('auto-list').children].find(card => card.dataset.ruleId === editing)?.querySelector('.auto-more');
    (trigger || $('auto-add')).focus({ preventScroll:true });
  });
  editor.addEventListener('click', event => {
    const rect = editor.getBoundingClientRect();
    if (event.target === editor && (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom)) closeEditor();
  });

  $('auto-enabled').addEventListener('click', () => mutate(c => { c.enabled = !c.enabled; }));

  $('auto-choose').addEventListener('click', async () => {

    try {

      const path = await invoke('choose_rule_application');

      if (path) { $('auto-path').value = path; if (!$('auto-name').value) $('auto-name').value = path.split(/[\\/]/).at(-1).replace(/\.exe$/i, ''); }

    } catch (error) { showError(error); }

  });

  $('auto-form').addEventListener('submit', async event => {

    event.preventDefault();

    const value = name => $(`auto-${name}`).value;

    const choice = name => value(name) === 'keep' ? null : value(name);

    const warpRoutes = value('mode') === 'proxy' && !useSavedRoutes ? { sites: [...selectedRoutes.sites], applications: [...selectedRoutes.applications] } : null;

    if (warpRoutes && (loadingRoutes || !routeCatalog)) { showError(t('auto_proxy_load_error')); return; }

    if (warpRoutes && !warpRoutes.sites.length && !warpRoutes.applications.length) { showError(t('auto_proxy_choose')); return; }

    if (warpRoutes && Object.keys(warpRoutes).some(k => warpRoutes[k].some(v => !routeCatalog[k].includes(v)))) { showError(t('auto_proxy_missing')); return; }

    const rule = { id: editing || crypto.randomUUID(), name: value('name').trim(), icon: selectedIcon, path: value('path'), enabled: config.rules.find(r => r.id === editing)?.enabled ?? true, zapret: value('zapret'), strategy: choice('strategy'), warp: value('warp'), warp_mode: choice('mode'), warp_routes: warpRoutes, restore: value('close') === 'restore', close_off: value('close') === 'off' };

    if (!rule.path || (!rule.strategy && !rule.warp_mode && rule.zapret === 'keep' && rule.warp === 'keep')) { showError(t('auto_required')); return; }

    if (await mutate(c => { const i = c.rules.findIndex(r => r.id === rule.id); if (i < 0) c.rules.push(rule); else c.rules[i] = rule; })) {

      $('auto-editor').close();

    }

  });

  document.addEventListener('zapret:section-changed', event => {

    menu.close(); sorter.cancel();

    Object.values(dropdowns).forEach(d => d.close());

    if (event.detail.sectionId === 'automations') refresh();

  });

  onLangChange(() => { $('auto-editor-title').textContent = t(editing ? 'auto_edit_rule' : 'auto_add'); sorter.cancel(); render(); renderIcons(); renderOptions(); showError(status.error); });

  refresh();

  setInterval(() => { if (!$('section-automations').classList.contains('hidden')) refresh(); }, 2500);

}

