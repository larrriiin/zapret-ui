// DesignBookmark Step Progress uses these values in steps.js, alongside its shared tokens.
const createSpring = (position, stiffness, damping) => ({ position, target: position, velocity: 0, stiffness, damping });
const clamp = value => Math.max(0, Math.min(1, value));
const CHECK_DELAY = 0.12;
const CHECK_LENGTH = 22;

function advance(spring, dt) {
  const force = spring.stiffness * (spring.target - spring.position) - spring.damping * spring.velocity;
  spring.velocity += force * dt;
  spring.position += spring.velocity * dt;
}

export function initSetupStepProgress(root) {
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const bar = root.querySelector('.setup-step-fill');
  const steps = Array.from(root.querySelectorAll('[data-setup-step]'), item => ({
    item,
    marker: item.querySelector('.setup-step-marker'),
    number: item.querySelector('.setup-step-number'),
    check: item.querySelector('path'),
    status: item.querySelector('[data-setup-complete]'),
    fill: createSpring(0, 260, 24),
    stroke: createSpring(0, 140, 20),
    pop: createSpring(1, 500, 14),
    active: createSpring(0, 300, 26),
    delay: 0,
  }));
  const rail = createSpring(0, 160, 22);
  const springs = [rail, ...steps.flatMap(({ fill, stroke, pop, active }) => [fill, stroke, pop, active])];
  let current = -1;
  let frame = 0;
  let lastTime = 0;

  function render() {
    bar.style.setProperty('--step-progress', clamp(rail.position).toFixed(5));
    for (const { marker, number, check, fill, stroke, pop, active } of steps) {
      const completed = clamp(fill.position);
      const selected = clamp(active.position);
      marker.style.background = completed > 0.01
        ? `color-mix(in srgb, var(--color-success) ${(completed * 100).toFixed(1)}%, var(--color-surface-container-highest))`
        : `color-mix(in srgb, var(--color-primary) ${(selected * 100).toFixed(1)}%, var(--color-surface-container-highest))`;
      marker.style.color = selected > 0.5 ? 'var(--color-on-primary)' : '';
      marker.style.transform = `scale(${pop.position.toFixed(4)})`;
      number.style.opacity = (1 - completed).toFixed(3);
      check.style.strokeDashoffset = (CHECK_LENGTH * (1 - clamp(stroke.position))).toFixed(2);
    }
  }

  function settle() {
    for (const step of steps) {
      if (step.delay > 0) step.stroke.target = 1;
      step.delay = 0;
    }
    for (const spring of springs) {
      spring.position = spring.target;
      spring.velocity = 0;
    }
    render();
  }

  function tick(now) {
    frame = 0;
    if (reducedMotion.matches) {
      settle();
      return;
    }
    const dt = Math.min((now - lastTime) / 1000, 1 / 30);
    lastTime = now;
    for (const step of steps) {
      if (step.delay <= 0) continue;
      step.delay -= dt;
      if (step.delay <= 0) step.stroke.target = 1;
    }
    // The reference integrates four smaller steps per frame for stable springs.
    for (let substep = 0; substep < 4; substep++) {
      for (const spring of springs) advance(spring, dt / 4);
    }
    const moving = springs.some(spring => Math.abs(spring.position - spring.target) > 0.001
      || Math.abs(spring.velocity) > 0.001);
    if (!moving && !steps.some(step => step.delay > 0)) {
      settle();
      return;
    }
    render();
    frame = requestAnimationFrame(tick);
  }

  return (index, animate) => {
    if (index === current && animate) return;
    current = index;
    const play = animate && !reducedMotion.matches;
    const finalStep = steps.length - 1;
    rail.target = index / finalStep;
    steps.forEach((step, position) => {
      const selected = position === index;
      const done = position < index || (selected && index === finalStep);
      step.item.toggleAttribute('data-complete', done);
      step.status.hidden = !done;
      if (selected) step.item.setAttribute('aria-current', 'step');
      else step.item.removeAttribute('aria-current');
      step.fill.target = Number(done);
      step.active.target = Number(selected && !done);
      if (!done || !play) {
        step.delay = 0;
        step.stroke.target = Number(done);
      } else if (step.stroke.target === 0 && step.delay <= 0) {
        step.delay = CHECK_DELAY;
      }
    });
    if (!play) {
      cancelAnimationFrame(frame);
      frame = 0;
      settle();
      return;
    }
    steps[index].pop.position = 0.7;
    steps[index].pop.velocity = 6;
    render();
    if (!frame) {
      lastTime = performance.now();
      frame = requestAnimationFrame(tick);
    }
  };
}
