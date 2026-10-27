// Selection and execution are distinct: persisted enabled is never proof of routing.
export function routingState(rule, { starting = false, error = null } = {}) {
  if (starting) return 'starting';
  if (error || rule?.error) return 'error';
  if (rule?.runtime_active) return 'active';
  return rule?.enabled ? 'configured' : 'off';
}
export function needsProxyConfirmation(snapshot) {
  return ['warp', 'warp+dot', 'warp+doh', 'tunnel_only'].includes(snapshot?.mode);
}
export function orderedWarpModes(modes) {
  return ['warp', 'proxy'].filter(mode => modes.includes(mode))
    .concat(modes.filter(mode => mode !== 'warp' && mode !== 'proxy'));
}
