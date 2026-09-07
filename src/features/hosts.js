import { $, invoke } from '../lib/core.js';
import { t, onLangChange } from '../lib/i18n.js';
import { markRestartIfServiceRunning } from '../lib/restart.js';

export function initHosts() {
  const button = $('hosts-update-btn');
  const status = $('hosts-update-status');
  let busy = false;
  let message = '';
  let params = {};
  const render = () => {
    button.disabled = busy;
    $('hosts-update-label').textContent = t(busy ? 'hosts_updating' : 'update_btn');
    $('hosts-update-icon').textContent = busy ? 'refresh' : 'download';
    $('hosts-update-icon').classList.toggle('animate-spin', busy);
    status.textContent = message ? t(message, params) : '';
    status.classList.toggle('hidden', !message);
  };
  onLangChange(render);
  button.addEventListener('click', async () => {
    if (busy) return;
    busy = true;
    message = 'hosts_updating';
    params = {};
    render();
    try {
      const result = await invoke('update_hosts');
      message = result.added ? 'hosts_updated' : 'hosts_current';
      params = { count: result.added, backup: result.backup };
      if (result.added > 0) await markRestartIfServiceRunning();
    } catch (error) {
      const known = ['admin_required', 'download_failed', 'invalid_source', 'unsupported_encoding',
        'open_failed', 'read_failed', 'file_too_large', 'backup_failed', 'write_failed', 'restore_failed'];
      message = known.includes(String(error)) ? `hosts_error_${error}` : 'hosts_error_write_failed';
    } finally {
      busy = false;
      render();
    }
  });
}
