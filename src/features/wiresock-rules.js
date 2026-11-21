import { invoke } from '../lib/core.js';
import { translateDOM, onLangChange, t } from '../lib/i18n.js';
import markup from '../components/wiresock-rules.html?raw';
import { renderRuleList } from './warp-rule-list.js';

export function initWireSockRules(root, { warpRules, openSites, openPath }, changed) {
  root.innerHTML = markup;
  translateDOM(root);
  const groups = [...root.querySelectorAll('[data-rules-group]')].map(group => ({
    key: group.dataset.rulesGroup,
    list: group.querySelector('[data-rules-list]'),
    add: group.querySelector('[data-rules-add]'),
    copy: group.querySelector('[data-rules-copy]'),
    empty: group.querySelector('[data-rules-empty]'),
  }));
  const message = root.querySelector('[data-rules-error]');
  let draft;
  let disabled = false;
  let picking = false;
  let revision = 0;

  function render() {
    for (const { key, list, add, copy, empty } of groups) {
      const values = draft[key];
      const locked = disabled || picking || values === null;
      add.disabled = locked;
      if (copy) copy.disabled = locked;
      empty.hidden = values === null || values.length > 0;
      renderRuleList(list, values || [], {
        kind: key === 'sites' ? 'sites' : 'apps', disabled: locked,
        details: key === 'sites' ? null : openPath,
        remove: value => {
          draft[key] = draft[key].filter(item => item !== value);
          update();
          add.focus();
        },
      });
    }
    root.querySelector('[data-rules-sites-help]').hidden = draft.sites === null;
    root.querySelector('[data-rules-sites-unavailable]').hidden = draft.sites !== null;
    root.querySelector('[data-rules-priority]').hidden = !draft.allowed.length || !draft.disallowed.length;
    root.querySelector('[data-rules-combined]').hidden = !draft.allowed.length || !draft.sites?.length;
  }

  function setMessage(key) {
    message.dataset.key = key || '';
    message.hidden = !key;
    message.textContent = key ? t(key) : '';
  }

  function update() {
    setMessage();
    render();
    changed();
  }

  function append(key, values) {
    if (disabled || !draft[key] || !values.length) return;
    const seen = new Set(draft[key].map(item => item.toLowerCase()));
    for (const value of values) {
      const folded = value.toLowerCase();
      if (seen.has(folded)) continue;
      seen.add(folded);
      draft[key].push(value);
    }
    update();
  }

  for (const { key, add, copy } of groups) {
    copy?.addEventListener('click', () => {
      const values = warpRules()[key];
      if (values.length) append(key, values);
      else setMessage(key === 'sites' ? 'wiresock_split_no_warp_sites' : 'wiresock_split_no_warp_apps');
    });
    add.addEventListener('click', async () => {
      if (key === 'sites') {
        openSites(sites => append(key, sites));
        return;
      }
      const current = revision;
      picking = true;
      render();
      try {
        const paths = await invoke('choose_warp_applications');
        if (current === revision && add.closest('dialog')?.open && Array.isArray(paths)) append(key, paths);
      } catch {
        if (current === revision) setMessage('warp_apps_invalid');
      } finally {
        if (current === revision) {
          picking = false;
          render();
          if (add.closest('dialog')?.open) add.focus();
        }
      }
    });
  }

  function reset({ applications = {}, sites = [] } = {}) {
    revision++;
    picking = false;
    draft = { allowed: [...(applications.allowed || [])], disallowed: [...(applications.disallowed || [])], sites: sites === null ? null : [...sites] };
    setMessage();
    render();
  }

  onLangChange(() => { render(); setMessage(message.dataset.key); });
  reset();
  return {
    reset,
    setDisabled(value) {
      if (disabled === value) return;
      disabled = value;
      render();
    },
    hasSites: () => Boolean(draft.sites?.length),
    value: () => ({ applications: { allowed: [...draft.allowed], disallowed: [...draft.disallowed] }, sites: [...(draft.sites || [])] }),
  };
}
