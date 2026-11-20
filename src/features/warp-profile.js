import { $, invoke } from '../lib/core.js';
import { t } from '../lib/i18n.js';

export function initWarpProfile() {
  const open = $('warp-profile-open');
  const dialog = $('warp-profile-dialog');
  const form = $('warp-profile-form');
  const fields = $('warp-profile-options');
  const advanced = $('warp-profile-advanced');
  const endpoint = $('warp-profile-endpoint');
  const randomEndpoint = $('warp-profile-random-endpoint');
  const masking = $('warp-profile-masking');
  const maskingOptions = $('warp-profile-masking-options');
  const maskingDomain = $('warp-profile-masking-domain');
  const split = $('warp-profile-split');
  const serviceGroup = $('warp-profile-services');
  const dns = $('warp-profile-dns');
  const lan = $('warp-profile-exclude-lan');
  const ipv6 = $('warp-profile-ipv6');
  const mtu = $('warp-profile-mtu');
  const keepalive = $('warp-profile-keepalive');
  const terms = $('warp-profile-terms');
  const save = $('warp-profile-save');
  const close = $('warp-profile-close');
  const reveal = $('warp-profile-reveal');
  const status = $('warp-profile-status');
  const error = $('warp-profile-error');
  let busy = false;
  let fileName = '';
  let opener = open;
  let onSaved = null;

  function selectedServices() {
    return Array.from(serviceGroup.querySelectorAll('input:checked'), input => input.value);
  }

  function updateControls() {
    fields.disabled = busy;
    save.disabled = busy || !terms.checked || (split.checked && selectedServices().length === 0);
    close.disabled = busy;
    reveal.hidden = !fileName;
    reveal.disabled = busy;
    endpoint.disabled = randomEndpoint.checked;
    maskingOptions.hidden = !masking.checked;
    maskingDomain.disabled = !masking.checked;
    serviceGroup.hidden = !split.checked;
    serviceGroup.disabled = !split.checked;
    dns.disabled = split.checked;
    $('warp-profile-system-dns').hidden = !split.checked;
    mtu.min = ipv6.checked ? '1280' : '576';
  }

  function setMessage(element, key, params) {
    element.textContent = key ? t(key, params) : '';
    element.hidden = !key;
  }

  function show(trigger = open, saved = null) {
    opener = trigger;
    onSaved = saved;
    form.reset();
    advanced.open = false;
    fileName = '';
    setMessage(status);
    setMessage(error);
    updateControls();
    dialog.showModal();
  }
  open.addEventListener('click', () => show());
  form.addEventListener('change', updateControls);
  form.addEventListener('invalid', event => {
    if (advanced.contains(event.target)) advanced.open = true;
  }, true);
  close.addEventListener('click', () => dialog.close());
  reveal.addEventListener('click', async () => {
    try {
      await invoke('reveal_warp_profile');
    } catch {
      setMessage(error, 'warp_profile_reveal_error');
    }
  });
  dialog.addEventListener('cancel', event => {
    if (busy) event.preventDefault();
  });
  dialog.addEventListener('close', () => opener.focus());

  form.addEventListener('submit', async event => {
    event.preventDefault();
    if (save.disabled) return;
    busy = true;
    updateControls();
    setMessage(error);
    setMessage(status, 'warp_profile_generating');
    try {
      fileName = await invoke('save_warp_profile', {
        options: {
          endpoint: endpoint.value,
          randomEndpoint: randomEndpoint.checked,
          maskingDomain: masking.checked ? maskingDomain.value : null,
          services: split.checked ? selectedServices() : null,
          dns: dns.querySelector('input:checked').value,
          excludeLan: lan.checked,
          ipv6: ipv6.checked,
          mtu: mtu.valueAsNumber,
          keepalive: keepalive.valueAsNumber,
        },
        acceptedTerms: terms.checked,
      });
      setMessage(status, 'warp_profile_saved', { fileName });
      onSaved?.(fileName);
    } catch (reason) {
      setMessage(status);
      const key = typeof reason === 'string' ? reason : '';
      setMessage(error, key.startsWith('warp_profile_') && t(key) !== key ? key : 'warp_profile_registration_error');
    } finally {
      busy = false;
      updateControls();
      (error.hidden ? reveal : save).focus();
    }
  });
  return show;
}
