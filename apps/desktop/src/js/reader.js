// The reader's side in the web view: keys, the wheel, a click on the page (it
// shows or hides the bars), how far down the strip is scrolled, and the bars
// that hide while reading.
// It reports what happened to the app with dioxus.send({t: ...}) and does what
// the app asks with {c: ...} messages. What a key or a tap means is decided in
// the app (logic/reader.rs); this only passes them on.
(() => {
  if (window.mpReaderStop) window.mpReaderStop();
  const root = () => document.getElementById('reader');
  const stage = () => document.querySelector('#reader .reader-stage');
  const KEYS = new Set([
    'ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', ' ', 'Home', 'End',
    '[', ']', '+', '=', '-', '_', '0', 'Escape', 'F11',
  ]);
  const typing = (el) =>
    !!el &&
    ((el.tagName === 'INPUT' && !['checkbox', 'radio', 'button', 'submit'].includes(el.type)) ||
      el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable);

  // ---- the bar ----
  let barTimer = null;
  const hideBar = () => {
    const r = root();
    if (!r) return;
    // Stays while the pointer is on it or a menu of it is open, and when the reader asked for it always.
    if (r.classList.contains('pinned')) return;
    if (r.querySelector('.reader-bar > *:hover') || document.querySelector('.menu-scrim, .reader-settings')) {
      barTimer = setTimeout(hideBar, 1500);
      return;
    }
    r.classList.remove('show-bar');
  };
  const showBar = () => {
    const r = root();
    if (!r) return;
    r.classList.add('show-bar');
    clearTimeout(barTimer);
    barTimer = setTimeout(hideBar, 2800);
  };
  const toggleBar = () => {
    const r = root();
    if (!r) return;
    if (r.classList.contains('show-bar')) {
      clearTimeout(barTimer);
      r.classList.remove('show-bar');
    } else {
      showBar();
    }
  };

  // ---- keys, wheel, taps ----
  const onKey = (e) => {
    if (!root() || typing(e.target) || e.altKey || e.metaKey) return;
    if (!KEYS.has(e.key)) return;
    if (e.ctrlKey && !['+', '=', '-', '0'].includes(e.key)) return;
    // Esc closes a menu or a dialog first (keys.js does that); only then leaves.
    if (e.key === 'Escape' && document.querySelector('.menu-scrim, .overlay, .backdrop')) return;
    // The reading settings are a page of their own: Esc goes back to the book.
    if (document.getElementById('reader-settings')) {
      if (e.key === 'Escape') { e.preventDefault(); document.getElementById('reader-settings-close')?.click(); }
      return;
    }
    // A button that has the focus keeps Space and Enter for itself.
    if (e.key === ' ' && e.target && e.target.tagName === 'BUTTON') return;
    e.preventDefault();
    showBar();
    dioxus.send({ t: 'key', k: e.key, shift: e.shiftKey, ctrl: e.ctrlKey });
  };
  const onWheel = (e) => {
    if (!root() || !e.ctrlKey) return;
    e.preventDefault();
    dioxus.send({ t: 'zoom', d: e.deltaY });
  };
  const onMove = () => showBar();
  const onClick = (e) => {
    const r = root();
    const s = stage();
    if (!r || !s || !s.contains(e.target)) return;
    if (e.target.closest('button, a, input, select, .reader-end, .reader-tile .retry')) return;
    toggleBar();
  };
  document.addEventListener('keydown', onKey, true);
  document.addEventListener('wheel', onWheel, { passive: false, capture: true });
  document.addEventListener('mousemove', onMove, true);
  document.addEventListener('click', onClick, true);
  showBar();

  // ---- how far down the strip is scrolled ----
  let lastReport = 0;
  let reportTimer = null;
  let lastKey = '';
  const report = () => {
    reportTimer = null;
    const r = root();
    const s = stage();
    if (!r || !s) return;
    const box = s.getBoundingClientRect();
    const mid = box.top + box.height * 0.35;
    const sections = [...s.querySelectorAll('.reader-chapter')];
    if (!sections.length) return;
    let current = sections[0];
    for (const sec of sections) {
      if (sec.getBoundingClientRect().top <= mid) current = sec;
    }
    const sb = current.getBoundingClientRect();
    const frac = sb.height > 0 ? Math.min(1, Math.max(0, (mid - sb.top) / sb.height)) : 0;
    const tiles = [...current.querySelectorAll('.reader-tile')];
    let page = 0;
    tiles.forEach((t, i) => {
      if (t.getBoundingClientRect().top < box.bottom - 8) page = i;
    });
    const remaining = s.scrollHeight - s.scrollTop - s.clientHeight;
    const message = {
      t: 'scroll',
      chapter: current.dataset.chapter,
      frac,
      page,
      pages: tiles.length,
      near_end: remaining < s.clientHeight * 2,
      end: remaining < 4,
    };
    const key = JSON.stringify(message);
    if (key === lastKey) return;
    lastKey = key;
    lastReport = Date.now();
    dioxus.send(message);
  };
  const onScroll = (e) => {
    const s = stage();
    if (!s || e.target !== s) return;
    // About four times a second while scrolling, and once more when it stops.
    if (Date.now() - lastReport > 250) report();
    else if (!reportTimer) reportTimer = setTimeout(report, 260);
  };
  document.addEventListener('scroll', onScroll, true);

  // ---- what the app asks for ----
  const restore = (chapter, frac, tries) => {
    const s = stage();
    const sec = s && s.querySelector('.reader-chapter[data-chapter="' + CSS.escape(chapter) + '"]');
    if (!sec || sec.offsetHeight === 0) {
      if (tries < 30) setTimeout(() => restore(chapter, frac, tries + 1), 50);
      return;
    }
    const top = sec.offsetTop + frac * sec.offsetHeight - s.clientHeight * 0.35;
    s.scrollTop = Math.max(0, top);
    lastKey = '';
    // Where it landed is told even if the strip did not move (a short chapter).
    setTimeout(report, 120);
  };
  const commands = {
    bar: () => toggleBar(),
    showBar: () => showBar(),
    restore: (m) => restore(m.chapter, m.frac, 0),
    top: () => {
      const s = stage();
      if (s) { s.scrollTop = 0; s.scrollLeft = 0; }
    },
    scrollBy: (m) => {
      const s = stage();
      if (!s) return;
      const dy = m.unit === 'page' ? s.clientHeight * 0.9 * m.n : 60 * m.n;
      s.scrollBy({ top: dy, behavior: 'smooth' });
    },
    scrollTo: (m) => {
      const s = stage();
      if (s) s.scrollTo({ top: m.where === 'bottom' ? s.scrollHeight : 0, behavior: 'smooth' });
    },
  };

  // The app calls this to ask for something (`{c: ...}`): a plain call from the app's side, which
  // reaches the page whatever the state of the message channel (that one only reports to the app).
  window.mpReaderCmd = (m) => {
    if (m && m.c === 'stop') {
      if (window.mpReaderStop) window.mpReaderStop();
      return;
    }
    const run = m && commands[m.c];
    if (run) run(m);
  };

  window.mpReaderStop = () => {
    document.removeEventListener('keydown', onKey, true);
    document.removeEventListener('wheel', onWheel, { capture: true });
    document.removeEventListener('mousemove', onMove, true);
    document.removeEventListener('click', onClick, true);
    document.removeEventListener('scroll', onScroll, true);
    clearTimeout(barTimer);
    clearTimeout(reportTimer);
    window.mpReaderStop = null;
    window.mpReaderCmd = null;
  };

  (async () => {
    try {
      for (;;) {
        const m = await dioxus.recv();
        if (m && m.c === 'stop') break;
        const run = m && commands[m.c];
        if (run) run(m);
      }
    } catch (_) {
      /* the screen was closed */
    }
    if (window.mpReaderStop) window.mpReaderStop();
  })();
})();
