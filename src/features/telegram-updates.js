import { invoke, listen } from '../lib/core.js';
import { t } from '../lib/i18n.js';

let updating = false;
export function isTelegramUpdating() { return updating; }
export async function checkTelegramUpdate() {
  const status = await invoke('get_telegram_status');
  if (!status.installed) return null;
  try { return await invoke('check_telegram_update'); }
  catch (error) { return { installed: true, current: status.version, available: false, error: String(error) }; }
}
export function mountTelegramUpdate(modal, info) {
  if (!info?.installed) return;
  const row = document.createElement('section');
  row.className = 'p-4 bg-white/5 rounded-2xl border border-white/5';
  row.setAttribute('aria-label', 'TG WS Proxy');
  row.innerHTML = `<div class="flex items-center justify-between"><div class="flex flex-col items-start text-left"><span class="text-[10px] font-bold text-primary/70 uppercase tracking-wider mb-1">TG WS Proxy</span><div class="flex items-center gap-2"><span data-tg-version class="text-sm font-bold text-on-surface"></span><span data-tg-arrow class="material-symbols-outlined text-xs text-on-surface-variant/40" aria-hidden="true">arrow_forward</span><span data-tg-latest class="text-sm font-bold text-primary"></span></div></div><div class="flex flex-col items-end gap-3"><span data-tg-badge></span><button id="modal-update-telegram-btn" type="button" class="inline-flex items-center justify-center gap-1.5 px-4 py-2 bg-primary/20 hover:bg-primary/30 border border-primary/20 rounded-xl text-[10px] font-black text-primary uppercase transition-all active:scale-95 shadow-lg shadow-primary/5"></button></div></div><p data-tg-status class="text-xs text-on-surface-variant mt-3" role="status" hidden></p><progress data-tg-progress class="module-progress mt-3" max="100" value="0" hidden aria-label="TG WS Proxy"></progress>`;
  modal.querySelector('#update-components').append(row);
  const label = row.querySelector('[data-tg-version]');
  const status = row.querySelector('[data-tg-status]');
  const button = row.querySelector('button');
  const progress = row.querySelector('progress');
  const badge = row.querySelector('[data-tg-badge]');
  const latest = row.querySelector('[data-tg-latest]');
  const arrow = row.querySelector('[data-tg-arrow]');
  function renderVersion() {
    label.textContent = `v${info.current}`;
    latest.textContent = info.available ? `v${info.latest}` : '';
    latest.hidden = arrow.hidden = !info.available;
    badge.className = info.available
      ? 'px-2 py-0.5 bg-primary/20 text-primary text-[10px] font-bold rounded-full uppercase'
      : 'text-on-surface-variant/50 text-[10px] font-bold uppercase';
    badge.textContent = t(info.error ? 'tg_update_check_failed' : info.available ? 'update_available_short' : 'up_to_date');
  }
  renderVersion();
  button.textContent = t(info.error ? 'tg_recheck' : 'update_now');
  button.hidden = !info.available && !info.error;
  button.addEventListener('click', async () => {
    if (updating) return;
    if (info.error) {
      button.disabled = true;
      try {
        const fresh = await checkTelegramUpdate();
        row.remove(); mountTelegramUpdate(modal, fresh);
      } catch { status.hidden = false; status.textContent = t('tg_update_check_failed'); }
      finally { button.disabled = false; }
      return;
    }
    updating = true;
    const controls = [...modal.querySelectorAll('button')].map(button => [button, button.disabled]);
    controls.forEach(([button]) => { button.disabled = true; });
    let unlisten;
    try {
      progress.hidden = false;
      status.hidden = false;
      status.textContent = t('tg_updating');
      unlisten = await listen('telegram-download-progress', ({payload}) => { progress.value = Number(payload); });
      const version = await invoke('update_telegram');
      info = { installed: true, current: version, latest: version, available: false };
      renderVersion();
      button.hidden = true;
      status.textContent = t('tg_update_done');
    } catch (error) {
      const [key, ...detail] = String(error).split(':');
      status.textContent = `${t(key)}${detail.length ? `: ${detail.join(':')}` : ''}`;
    } finally {
      unlisten?.(); progress.hidden = true; updating = false;
      controls.forEach(([button, disabled]) => { button.disabled = disabled; });
    }
  });
}
