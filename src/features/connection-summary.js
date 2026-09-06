import { $ } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { state } from '../lib/state.js';

let zapret = { running: false };
let warpConnected = false;

export function connectionLabel(status, warp, fallback) {
  const parts = [];
  if (status.running) parts.push(status.strategy || fallback);
  if (warp) parts.push('WARP');
  return parts.join(' + ');
}

function render() {
  if (state.restartInProgress) return;
  const header = $('header-status');
  if (!header) return;
  const label = connectionLabel(zapret, warpConnected, t('status_connected'));
  const prefix = document.createElement('span');
  prefix.className = 'text-primary';
  prefix.textContent = `${t('status_label')}: `;
  const value = document.createElement('span');
  value.className = label ? 'text-secondary' : 'text-error-dim';
  value.textContent = label || t('status_disconnected');
  header.replaceChildren(prefix, value);
}

export function setZapretSummary(status) { zapret = status; render(); }
export function setWarpSummary(connected) { warpConnected = connected; render(); }
onLangChange(render);
