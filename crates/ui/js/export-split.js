// Dragging the bars between the panels and the edges of the side columns of the Export screen.
// The sizes are remembered between visits.
// Shared by both apps (`mangapoint_ui::scripts`): `MP.export_split.start()` wires the Export
// screen on the page now (once for each screen).
(function () {
  const MP = (window.MP = window.MP || {});
  MP.export_split = {
    start() {
      const root = [...document.querySelectorAll('.export-screen')].find((r) => !r.dataset.dragBound);
      // (Not `data-split`: that names the bars, and the screen itself must not match them.)
      if (root && !root.dataset.dragBound) {
       root.dataset.dragBound = '1';
       const KEY = 'mp-export-layout';
       let saved = {};
       try { saved = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch (e) {}
       const body = root.querySelector('.export-body');
       // (The narrow layout has no columns, so no body to size.)
       if (body && saved.left) body.style.setProperty('--left-w', saved.left + 'px');
       if (body && saved.right) body.style.setProperty('--right-w', saved.right + 'px');
       for (const [id, h] of Object.entries(saved.panels || {})) {
        const el = root.querySelector('[data-panel="' + id + '"]');
        if (el) el.style.setProperty('--h', h + 'px');
       }
       // A saved layout that is too tall for this window (smaller, or its panels dragged bigger before) is
       // squeezed back so every open panel keeps its minimum, instead of pushing the others about.
       const fit = () => {
        const MIN = 160;
        for (const side of root.querySelectorAll('.export-side')) {
         const panels = [...side.querySelectorAll(':scope > .px-panel')];
         const fixed = panels.filter((p) => p.classList.contains('open') && p.classList.contains('fixed'));
         const flexible = panels.filter((p) => p.classList.contains('open') && !p.classList.contains('fixed')).length;
         const shut = panels.filter((p) => p.classList.contains('collapsed')).length;
         const wanted = fixed.map((p) => parseFloat(p.style.getPropertyValue('--h')) || MIN);
         const budget = side.clientHeight - MIN * flexible - 40 * shut - 8;
         const total = wanted.reduce((a, b) => a + b, 0);
         if (!fixed.length || total <= budget) continue;
         fixed.forEach((p, i) => p.style.setProperty('--h', Math.max(MIN, Math.floor(wanted[i] * budget / total)) + 'px'));
        }
       };
       fit();
       window.addEventListener('resize', fit);
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
         drag = { kind, bar, el, id: bar.dataset.target, from: e.clientY, start: el.getBoundingClientRect().height, dir: Number(bar.dataset.dir),
          other: (() => { let n = Number(bar.dataset.dir) < 0 ? bar.previousElementSibling : bar.nextElementSibling; while (n && !n.matches('.px-panel')) n = Number(bar.dataset.dir) < 0 ? n.previousElementSibling : n.nextElementSibling; return n && !n.classList.contains('collapsed') ? n.getBoundingClientRect().height : 0; })() };
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
         // A bar moves only the line between the two panels beside it: neither is made smaller than
         // MIN (the same for every panel), so a panel never pushes another one out of the way.
         const MIN = 160;
         const most = drag.other ? drag.start + drag.other - MIN : drag.start;
         const h = Math.min(Math.max(MIN, most), Math.max(MIN, Math.round(drag.start + drag.dir * (e.clientY - drag.from))));
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
    },
  };
})();
