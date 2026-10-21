const root = document.querySelector('.export-screen');
// (Not `data-split`: that names the bars, and the screen itself must not match them.)
if (root && !root.dataset.dragBound) {
 root.dataset.dragBound = '1';
 const KEY = 'mp-export-layout';
 let saved = {};
 try { saved = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch (e) {}
 const body = root.querySelector('.export-body');
 if (saved.left) body.style.setProperty('--left-w', saved.left + 'px');
 if (saved.right) body.style.setProperty('--right-w', saved.right + 'px');
 for (const [id, h] of Object.entries(saved.panels || {})) {
  const el = root.querySelector('[data-panel="' + id + '"]');
  if (el) el.style.setProperty('--h', h + 'px');
 }
 let drag = null;
 root.addEventListener('pointerdown', e => {
  const bar = e.target.closest('.px-split, .px-split-v');
  if (!bar || !root.contains(bar)) return;
  e.preventDefault();
  try { bar.setPointerCapture(e.pointerId); } catch (err) {}
  const kind = bar.dataset.split;
  if (kind === 'panel') {
   const el = root.querySelector('[data-panel="' + bar.dataset.target + '"]');
   if (!el) return;
   drag = { kind, bar, el, id: bar.dataset.target, from: e.clientY, start: el.getBoundingClientRect().height, dir: Number(bar.dataset.dir) };
  } else {
   const side = root.querySelector('.export-side.' + kind);
   drag = { kind, bar, from: e.clientX, start: side.getBoundingClientRect().width };
  }
  bar.classList.add('active');
  root.classList.add('resizing');
 });
 root.addEventListener('pointermove', e => {
  if (!drag) return;
  if (drag.kind === 'panel') {
   // No panel grows over the others: each of them keeps room for its title bar and a few rows.
   const side = drag.el.parentElement;
   let others = 0;
   for (const p of side.querySelectorAll(':scope > .px-panel')) {
    if (p !== drag.el) others += p.classList.contains('collapsed') ? 40 : p.classList.contains('fixed') ? 100 : 160;
   }
   const most = Math.max(60, side.clientHeight - others);
   const h = Math.min(most, Math.max(60, Math.round(drag.start + drag.dir * (e.clientY - drag.from))));
   drag.el.style.setProperty('--h', h + 'px');
   saved.panels = Object.assign({}, saved.panels, { [drag.id]: h });
  } else {
   const dx = e.clientX - drag.from;
   const w = Math.max(220, Math.min(620, Math.round(drag.start + (drag.kind === 'left' ? dx : -dx))));
   body.style.setProperty(drag.kind === 'left' ? '--left-w' : '--right-w', w + 'px');
   saved[drag.kind] = w;
  }
 });
 const stop = () => {
  if (!drag) return;
  drag.bar.classList.remove('active');
  root.classList.remove('resizing');
  drag = null;
  try { localStorage.setItem(KEY, JSON.stringify(saved)); } catch (e) {}
 };
 root.addEventListener('pointerup', stop);
 root.addEventListener('pointercancel', stop);
}
