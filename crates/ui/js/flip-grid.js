// Lets the cards of the Library slide to their new places when one leaves or arrives, instead of
// jumping. Shared by both apps (`mangapoint_ui::scripts`): `MP.flip_grid.start()` watches the grid.
(function () {
  const MP = (window.MP = window.MP || {});
  MP.flip_grid = {
    start() {
      const grid = document.querySelector('.poster-grid');
      if (!grid || grid.__flip) return;
      grid.__flip = true;
      const where = new Map();
      const read = () => {
        for (const el of grid.children) where.set(el.dataset.id, [el.offsetLeft, el.offsetTop]);
      };
      const slide = () => {
        const calm = matchMedia('(prefers-reduced-motion: reduce)').matches || !!document.querySelector('.app[data-motion="snappy"]');
        const seen = new Set();
        for (const el of grid.children) {
          const id = el.dataset.id;
          seen.add(id);
          const was = where.get(id);
          const now = [el.offsetLeft, el.offsetTop];
          if (!calm && was && !el.classList.contains('leaving') && (was[0] !== now[0] || was[1] !== now[1])) {
            el.animate(
              [{ translate: `${was[0] - now[0]}px ${was[1] - now[1]}px` }, { translate: '0 0' }],
              { duration: 380, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)' }
            );
          }
          where.set(id, now);
        }
        for (const id of [...where.keys()]) if (!seen.has(id)) where.delete(id);
      };
      new MutationObserver(slide).observe(grid, { childList: true });
      new ResizeObserver(read).observe(grid);
      read();
    },
  };
})();
