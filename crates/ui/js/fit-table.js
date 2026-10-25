// Sizes the chapter table so that the page around it does not scroll: the table takes what is left
// of the window under it, and it is the only thing on the page that scrolls. Shared by both apps
// (`mangapoint_ui::scripts`): `MP.fit_table.start()` keeps the size up to date from then on.
(function () {
  const MP = (window.MP = window.MP || {});
  MP.fit_table = {
    start() {
      if (window.__fitTable) return window.__fitTable();
      const fit = () => {
        const table = [...document.querySelectorAll('.table-scroll')].find((el) => el.offsetParent && !el.closest('.modal, .overlay'));
        const root = document.documentElement;
        if (!table) return root.style.removeProperty('--fit-h');
        const scroller = document.querySelector('.content');
        const top = table.getBoundingClientRect().top + (scroller ? scroller.scrollTop : 0);
        // 140px keeps clear of the bottom bar, as the page's own padding does.
        root.style.setProperty('--fit-h', Math.max(220, Math.floor(innerHeight - top - 144)) + 'px');
      };
      window.__fitTable = fit;
      addEventListener('resize', fit);
      let queued = false;
      new MutationObserver(() => {
        if (queued) return;
        queued = true;
        requestAnimationFrame(() => {
          queued = false;
          fit();
        });
      }).observe(document.body, { childList: true, subtree: true });
      fit();
    },
  };
})();
