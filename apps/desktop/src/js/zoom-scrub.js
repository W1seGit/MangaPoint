// A drag handle for a zoom box: press on it and move left or right to change
// the number, without the pointer ever running out of room (the pointer is
// locked, so it can travel as far as the hand does). Shift is fine steps and
// Alt is coarse ones. The box shows the number as it moves, and every
// fraction of a second the new value is sent on, as if it had been typed, so
// what the zoom does can be watched while dragging.
if (!window.__mpScrub) {
  window.__mpScrub = true;
  let drag = null;
  const read = (input, min) => {
    const t = input.value.trim().toLowerCase();
    const n = parseFloat(t);
    return t.startsWith('fit') || !isFinite(n) ? Math.max(100, min) : n;
  };
  const send = d => {
    d.sent = d.input.value;
    d.at = performance.now();
    d.input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const finish = commit => {
    if (!drag) return;
    const d = drag;
    drag = null;
    document.body.classList.remove('is-scrubbing');
    d.handle.classList.remove('active');
    if (document.pointerLockElement === d.handle) document.exitPointerLock();
    if (commit && d.moved) { if (d.input.value !== d.sent) send(d); }
    else if (!commit) {
      d.input.value = d.before;
      if (d.sent && d.sent !== d.before) d.input.dispatchEvent(new Event('change', { bubbles: true }));
    }
  };
  document.addEventListener('pointerdown', e => {
    const handle = e.target.closest && e.target.closest('[data-scrub]');
    if (!handle || e.button !== 0) return;
    const input = handle.closest('.zoom-field').querySelector('input');
    const min = Number(handle.dataset.min) || 25, max = Number(handle.dataset.max) || 400;
    drag = { handle, input, min, max, value: read(input, min), before: input.value, moved: false, last: e.clientX, locked: false, sent: '', at: 0 };
    handle.classList.add('active');
    document.body.classList.add('is-scrubbing');
    e.preventDefault();
    try { const p = handle.requestPointerLock(); if (p && p.catch) p.catch(() => {}); } catch (_) {}
  });
  document.addEventListener('pointerlockchange', () => {
    if (!drag) return;
    if (document.pointerLockElement === drag.handle) drag.locked = true;
    else if (drag.locked) finish(true);
  });
  document.addEventListener('pointermove', e => {
    if (!drag) return;
    const step = e.shiftKey ? 0.25 : e.altKey ? 5 : 1;
    const dx = drag.locked || e.movementX ? e.movementX : e.clientX - drag.last;
    drag.last = e.clientX;
    if (!dx) return;
    drag.value = Math.min(drag.max, Math.max(drag.min, drag.value + dx * step));
    drag.moved = true;
    drag.input.value = Math.round(drag.value) + '%';
    if (performance.now() - drag.at >= 140 && drag.input.value !== drag.sent) send(drag);
  });
  document.addEventListener('pointerup', () => finish(true));
  document.addEventListener('pointercancel', () => finish(false));
  document.addEventListener('keydown', e => { if (drag && e.key === 'Escape') finish(false); });
}
