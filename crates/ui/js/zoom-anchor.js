// Zooming the reader (Ctrl and + or -, the wheel, a pinch, the menu) keeps the reader on the same spot:
// the stage is scrolled by the same factor the pages grew or shrank by, around the middle of the view.
// Shared by both apps (`mangapoint_ui::scripts`): `MP.zoom_anchor.start()` begins watching, once.
(function () {
  const MP = (window.MP = window.MP || {});
  if (MP.zoom_anchor) return;
  let started = false;
  const stageOf = (reader) => reader.querySelector('.reader-stage');
  const zoomOf = (reader) => {
    const z = parseFloat(reader.style.getPropertyValue('--zoom'));
    return Number.isFinite(z) && z > 0 ? z : 1;
  };
  MP.zoom_anchor = {
    start() {
      if (started) return;
      started = true;
      let snap = null;
      const remember = () => {
        const reader = document.getElementById('reader');
        const stage = reader && stageOf(reader);
        snap = stage ? { z: zoomOf(reader), top: stage.scrollTop, left: stage.scrollLeft } : null;
      };
      document.addEventListener('scroll', (e) => {
        if (e.target && e.target.classList && e.target.classList.contains('reader-stage')) remember();
      }, true);
      new MutationObserver((records) => {
        for (const r of records) {
          if (!r.target || r.target.id !== 'reader') continue;
          const reader = r.target;
          const stage = stageOf(reader);
          if (!stage) continue;
          const z = zoomOf(reader);
          if (!snap) { remember(); continue; }
          if (z === snap.z) continue;
          const k = z / snap.z;
          const w = stage.clientWidth;
          const h = stage.clientHeight;
          stage.scrollTop = Math.max(0, (snap.top + h / 2) * k - h / 2);
          stage.scrollLeft = Math.max(0, (snap.left + w / 2) * k - w / 2);
          remember();
        }
      }).observe(document.body, { attributes: true, subtree: true, attributeFilter: ['style'] });
    },
  };
})();
