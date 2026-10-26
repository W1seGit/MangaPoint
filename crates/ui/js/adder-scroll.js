// Keeps the row Enter would take in the reader adder in view when the list is long. Only the list
// scrolls: scrollIntoView would also scroll every page around it, and the first-run page that is
// still sliding in sits far off to the side.
// Shared by both apps (`mangapoint_ui::scripts`): `MP.adder_scroll.start()` runs it once.
(function () {
  const MP = (window.MP = window.MP || {});
  MP.adder_scroll = {
    start() {
      const o = document.querySelector('.adder-opt.active');
      const l = o && o.closest('.adder-list');
      if (!l) return;
      const lr = l.getBoundingClientRect();
      const r = o.getBoundingClientRect();
      if (r.top < lr.top) l.scrollTop += r.top - lr.top;
      else if (r.bottom > lr.bottom) l.scrollTop += r.bottom - lr.bottom;
    },
  };
})();
