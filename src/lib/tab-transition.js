// Keep one interruptible transition per tab group; the latest selection wins.
const transitions = new WeakMap();

export function transitionTabContent(container, swap) {
  const opacity = getComputedStyle(container).opacity;
  transitions.get(container)?.cancel();
  transitions.delete(container);
  if (!container.getClientRects().length || matchMedia('(prefers-reduced-motion: reduce)').matches) {
    swap();
    return;
  }
  const style = getComputedStyle(container);
  const milliseconds = name => {
    const value = style.getPropertyValue(name).trim();
    return parseFloat(value) * (value.endsWith('ms') ? 1 : 1000);
  };
  const easing = style.getPropertyValue('--ui-ease-out').trim();
  const outgoing = container.animate([{ opacity }, { opacity: 0 }], {
    duration: milliseconds('--ui-duration-exit'), easing, fill: 'forwards',
  });
  transitions.set(container, outgoing);
  outgoing.finished.then(() => {
    if (transitions.get(container) !== outgoing) return;
    swap();
    outgoing.cancel();
    const incoming = container.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: milliseconds('--ui-duration'), easing,
    });
    transitions.set(container, incoming);
    incoming.finished.then(() => {
      if (transitions.get(container) === incoming) transitions.delete(container);
    }).catch(() => {}); // Rapid tab changes cancel the previous fade.
  }).catch(() => {});
}
