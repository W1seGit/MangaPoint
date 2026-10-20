let tries = 0;
const start = () => {
 // The editor that is not wired yet: switching between the converted pages and the original ones
 // replaces the element, and for a moment the old one is still there.
 const root = [...document.querySelectorAll('#page-editor')].find(r => !r.dataset.bound);
 if (!root) { if (tries++ < 120) requestAnimationFrame(start); return; }
 root.dataset.bound = '1';
 const scroller = root.querySelector('.editor-scroll');
 const page = root.closest('.content') || document.documentElement;
 let frame = 0, last = '';
 const vars = {};
 const set = (name, value) => { if (vars[name] !== value) { vars[name] = value; root.style.setProperty(name, value); } };
 // The viewer is as tall as the space left under the series header, so the
 // whole page does not scroll. Its position is measured inside the scrolling
 // page: measured on the screen it would change as the page scrolls, and the
 // viewer would grow every time it was scrolled.
 const fit = () => {
  const top = scroller.getBoundingClientRect().top - page.getBoundingClientRect().top + page.scrollTop;
  const bar = root.querySelector('.editor-bottom');
  const below = (bar ? bar.offsetHeight : 0) + (parseFloat(getComputedStyle(page).paddingBottom) || 0) + 8;
  set('--canvas-height', Math.max(240, Math.round(page.clientHeight - top - below)) + 'px');
  set('--bottom-height', (bar ? bar.offsetHeight : 0) + 'px');
  const style = getComputedStyle(scroller);
  const room = scroller.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom) - 96;
  set('--reader-page-height', Math.max(120, Math.round(room)) + 'px');
  const pane = root.querySelector('.row-pane.result-pane');
  if (pane && pane.clientWidth > 0) set('--reader-page-width', pane.clientWidth + 'px');
 };
 const report = () => {
  const box = scroller.getBoundingClientRect();
  const probe = box.top + box.height * 0.3;
  let lo = -1, hi = -1, cur = -1;
  for (const row of scroller.querySelectorAll('.editor-row, .original-page')) {
   const b = row.getBoundingClientRect(), i = Number(row.dataset.row);
   if (b.bottom >= box.top - box.height * 0.5 && b.top <= box.bottom + box.height * 0.5) {
    if (lo < 0) lo = i;
    hi = i;
   }
   if (cur < 0 && b.bottom > probe) cur = i;
  }
  const message = lo + ',' + hi + ',' + cur;
  if (message !== last) { last = message; dioxus.send([lo, hi, cur]); }
 };
 const run = () => { frame = 0; if (root.isConnected) { fit(); report(); } };
 const later = () => { if (!frame) frame = requestAnimationFrame(run); };
 scroller.addEventListener('scroll', later, { passive: true });
 window.addEventListener('resize', later);
 new ResizeObserver(later).observe(scroller);
 new ResizeObserver(later).observe(page);
 new MutationObserver(later).observe(scroller, { childList: true, subtree: true });
 // Things above the viewer (the series header, a cover) can still be loading.
 const timer = setInterval(() => { if (!root.isConnected) clearInterval(timer); else later(); }, 400);
 root.addEventListener('keydown', e => {
  if (e.target.closest('input,select,textarea,[contenteditable=true]')) return;
  const ids = { Escape: 'editor-close-overview' };
  const id = ids[e.key];
  if (id) { const b = document.getElementById(id); if (b && !b.disabled) { e.preventDefault(); b.click(); } }
 });
 later();
};
start();
// Stay alive: messages can only be sent while the script has not finished.
await new Promise(() => {});
