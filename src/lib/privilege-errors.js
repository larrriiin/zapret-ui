const reasons = new Set(['service', 'traffic', 'tests', 'hosts', 'core', 'files', 'warp_apps']);

export function adminReason(error) {
  if (error?.code === 'admin_required') return reasons.has(error.detail) ? error.detail : 'service';
  const match = String(error?.message || error).match(/admin_required:\s*(service|traffic|tests|hosts|core|files|warp_apps)\b/);
  return match?.[1] || (error?.code === 'warp_apps_admin' ? 'warp_apps' : null);
}
