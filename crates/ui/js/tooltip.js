// One floating tip for every question mark. It sits on the page itself, so a
// panel that scrolls or clips its contents can neither cut it off nor grow a
// scroll bar because of it.
// Shared by both apps (`mangapoint_ui::scripts`): `MP.tooltip.start()` makes the tip, once.
(function () {
  const MP = (window.MP = window.MP || {});
  if (MP.tooltip) return;
  let started = false;
  MP.tooltip = {
    start() {
      if (started) return;
      started = true;
      const tip = document.createElement('div');
      tip.className = 'floating-tip';
      document.body.appendChild(tip);
      let current = null;
      const hide = () => { current = null; tip.classList.remove('shown'); };
      const show = (el) => {
        current = el;
        tip.textContent = el.getAttribute('data-tip') || '';
        tip.classList.add('shown');
        const r = el.getBoundingClientRect();
        const w = tip.offsetWidth, h = tip.offsetHeight, gap = 10, edge = 8;
        let left = r.right + gap;
        if (left + w > window.innerWidth - edge) left = r.left - gap - w;
        left = Math.max(edge, Math.min(left, window.innerWidth - w - edge));
        let top = r.top + r.height / 2 - h / 2;
        top = Math.max(edge, Math.min(top, window.innerHeight - h - edge));
        tip.style.left = left + 'px';
        tip.style.top = top + 'px';
      };
      const find = (e) => (e.target && e.target.closest) ? e.target.closest('.help[data-tip]') : null;
      document.addEventListener('mouseover', (e) => {
        const el = find(e);
        if (el === current) return;
        if (el) show(el); else hide();
      });
      document.addEventListener('focusin', (e) => { const el = find(e); if (el) show(el); });
      document.addEventListener('focusout', hide);
      document.addEventListener('scroll', hide, true);
    },
  };
})();
