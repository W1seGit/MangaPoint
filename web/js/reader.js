// The reader's page glue: it fills the empty page tiles the Rust UI draws,
// watches the keyboard, taps, swipes and pinches, and tells the UI where the
// reader is. The UI decides what any of that means.
import { acquire, release, rememberSize, source, knownRatio, pin } from './pages.js';
import { imageSize } from './imagesize.js';

let emit = () => {};

// ---- tiles ----------------------------------------------------------------------------

const live = new Map(); // tile -> { book, index, url, img }
let io = null;

function tileOf(el) {
  return el.classList && el.classList.contains('reader-tile') ? el : null;
}

/** The tile's shape: the CSS uses it before the picture has arrived. */
function setShape(tile, w, h) {
  tile.style.setProperty('--ar', `${w} / ${h}`);
  tile.style.setProperty('--arn', String(w / h));
  tile.style.setProperty('--w', `${w}px`);
}

// Pictures are opened a few at a time, the ones nearest the screen first (those just below it
// before those above it), so a far-off page never holds up the one about to be seen.
const WORKERS = 4;
let running = 0;
const waiting = new Map(); // tile -> resolve

function thumbDistance(tile) {
  const r = tile.getBoundingClientRect();
  return r.bottom < 0 ? -r.bottom : r.top > innerHeight ? r.top - innerHeight : 0;
}

function distance(tile) {
  // The small pictures beside a page list wait for the pages themselves.
  if (tile.classList.contains('original-thumb')) return 1e6 + thumbDistance(tile);
  const r = tile.getBoundingClientRect();
  if (r.bottom < 0) return -r.bottom * 2.5;
  if (r.top > innerHeight) return r.top - innerHeight;
  return 0;
}

function pump() {
  while (running < WORKERS && waiting.size) {
    let best = null;
    let bestD = Infinity;
    for (const t of waiting.keys()) {
      const d = distance(t);
      if (d < bestD) { best = t; bestD = d; }
    }
    const go = waiting.get(best);
    waiting.delete(best);
    running++;
    go();
  }
}

function turn(tile) {
  return new Promise((go) => {
    waiting.set(tile, go);
    pump();
  });
}

function done() {
  running--;
  pump();
}

// Every page of a chapter gets its true shape from the start of its file, nearest the screen
// first, so the strip has its real height before anything is opened and nothing jumps.
const measuring = new Set();
let measureRunning = 0;

function measurePump() {
  while (measureRunning < 6 && measuring.size) {
    let best = null;
    let bestD = Infinity;
    for (const t of measuring) {
      if (!t.isConnected || t.dataset.loaded || t.dataset.sized) { measuring.delete(t); continue; }
      const d = distance(t);
      if (d < bestD) { best = t; bestD = d; }
    }
    if (!best) return;
    measuring.delete(best);
    measureRunning++;
    measureTile(best).finally(() => { measureRunning--; measurePump(); });
  }
}

async function measureTile(tile) {
  try {
    const book = tile.dataset.book;
    const index = Number(tile.dataset.page);
    const src = await source(book);
    const p = src.table.pages[index];
    if (p && p.w > 0 && p.h > 0) return;
    const dims = imageSize(await src.start(index));
    if (!dims || !(dims[0] > 0 && dims[1] > 0)) return;
    tile.dataset.sized = '1';
    if (!tile.dataset.loaded) setShape(tile, dims[0], dims[1]);
    rememberSize(book, index, dims[0], dims[1]);
  } catch (_) { /* it is measured when it is opened */ }
}

async function fillTile(tile) {
  const book = tile.dataset.book;
  const index = Number(tile.dataset.page);
  if (live.has(tile)) return;
  const rec = { book, index, url: null, img: null, gone: false };
  live.set(tile, rec);
  try {
    const src = await source(book);
    const ratio = knownRatio(src, index);
    if (ratio) {
      setShape(tile, ratio.w, ratio.h);
    }
    await turn(tile);
    let url;
    try {
      if (rec.gone || live.get(tile) !== rec) return;
      url = await acquire(book, index);
    } finally {
      done();
    }
    if (rec.gone || live.get(tile) !== rec) {
      release(book, index);
      return;
    }
    rec.url = url;
    const img = new Image();
    img.alt = `Page ${index + 1}`;
    img.draggable = false;
    img.decoding = 'async';
    img.onload = () => {
      if (rec.gone) return;
      tile.classList.remove('failed');
      setShape(tile, img.naturalWidth, img.naturalHeight);
      tile.dataset.loaded = '1';
      rememberSize(book, index, img.naturalWidth, img.naturalHeight);
    };
    img.onerror = () => {
      if (!rec.gone) showFailure(tile, rec, index);
    };
    rec.img = img;
    img.src = url;
    // Shown once it is ready to paint, so a page never appears half drawn.
    try { await img.decode(); } catch (_) { return; /* onerror shows it */ }
    if (rec.gone) return;
    tile.appendChild(img);
  } catch (err) {
    live.delete(tile);
    if (!rec.gone) showFailure(tile, rec, index, err);
  }
}

function showFailure(tile, rec, index, err) {
  tile.classList.add('failed');
  tile.querySelector('img')?.remove();
  if (tile.querySelector('.reader-fail')) return;
  const box = document.createElement('div');
  box.className = 'reader-fail';
  const msg = document.createElement('span');
  msg.textContent = `Page ${index + 1} could not be shown.`;
  box.appendChild(msg);
  if (err && err.message) {
    const why = document.createElement('span');
    why.className = 'muted small';
    why.textContent = err.message;
    box.appendChild(why);
  }
  const retry = document.createElement('button');
  retry.className = 'btn small retry';
  retry.textContent = 'Retry';
  retry.setAttribute('aria-label', `Retry page ${index + 1}`);
  retry.addEventListener('click', (e) => {
    e.stopPropagation();
    box.remove();
    unfillTile(tile);
    fillTile(tile);
  });
  box.appendChild(retry);
  tile.appendChild(box);
}

function unfillTile(tile) {
  const rec = live.get(tile);
  if (!rec) return;
  rec.gone = true;
  live.delete(tile);
  waiting.delete(tile);
  if (rec.img) {
    rec.img.onload = rec.img.onerror = null;
    rec.img.removeAttribute('src');
    rec.img.remove();
  }
  tile.querySelector('.reader-fail')?.remove();
  tile.classList.remove('failed');
  delete tile.dataset.loaded;
  if (rec.url) release(rec.book, rec.index);
}

function ensureObserver() {
  if (io) return;
  io = new IntersectionObserver(
    (changes) => {
      for (const c of changes) {
        if (c.isIntersecting) fillTile(c.target);
        else unfillTile(c.target);
      }
    },
    // Well ahead of the screen (and some behind), so the next pages are there before they are scrolled to.
    { rootMargin: '600% 0px 800% 0px' },
  );
}

/** Shape every tile from the sizes already measured, so the strip has its true height at once. */
function shapeTiles(tiles) {
  const byBook = new Map();
  for (const t of tiles) {
    if (!byBook.has(t.dataset.book)) byBook.set(t.dataset.book, []);
    byBook.get(t.dataset.book).push(t);
  }
  for (const [book, list] of byBook) {
    source(book)
      .then((src) => {
        for (const t of list) {
          if (t.dataset.loaded || !t.isConnected) continue;
          const ratio = knownRatio(src, Number(t.dataset.page));
          if (ratio) setShape(t, ratio.w, ratio.h);
        }
      })
      .catch(() => {});
  }
}

function scan(node) {
  if (node.nodeType !== 1) return;
  const tiles = tileOf(node) ? [node] : node.querySelectorAll ? [...node.querySelectorAll('.reader-tile[data-book]')] : [];
  if (tiles.length) shapeTiles(tiles);
  for (const t of tiles) {
    ensureObserver();
    io.observe(t);
    measuring.add(t);
  }
  measurePump();
}

function unscan(node) {
  if (node.nodeType !== 1) return;
  const tiles = tileOf(node) ? [node] : node.querySelectorAll ? [...node.querySelectorAll('.reader-tile')] : [];
  for (const t of tiles) {
    io?.unobserve(t);
    unfillTile(t);
  }
}

// ---- covers: <img data-cover="book id"> is filled from the stored thumbnail ----------------

let coverLoader = null;
export function setCoverLoader(fn) {
  coverLoader = fn;
}

async function fillCover(el) {
  if (!coverLoader || el.dataset.coverDone === el.dataset.cover) return;
  const id = el.dataset.cover;
  el.dataset.coverDone = id;
  const url = await coverLoader(id);
  if (url && el.dataset.cover === id) {
    el.src = url;
    el.classList.add('has-cover');
  } else {
    el.classList.add('no-cover-img');
  }
}

function scanCovers(node) {
  if (node.nodeType !== 1) return;
  const list = node.matches?.('[data-cover]') ? [node] : [];
  if (node.querySelectorAll) list.push(...node.querySelectorAll('[data-cover]'));
  for (const el of list) fillCover(el);
}

// ---- keyboard ------------------------------------------------------------------------------

const HANDLED = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ', 'Spacebar', '[', ']', '+', '=', '-', '_', '0', 'Escape', 'F11']);

function onKey(e) {
  const reader = document.getElementById('reader');
  if (!reader) return;
  const t = e.target;
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return;
  // A focused button keeps Enter/Space as its own click.
  if (t && t.tagName === 'BUTTON' && (e.key === ' ' || e.key === 'Enter')) return;
  if (e.altKey || e.metaKey) return;
  if (e.ctrlKey && !['+', '=', '-', '0'].includes(e.key)) return;
  if (!HANDLED.has(e.key)) return;
  e.preventDefault();
  emit({ t: 'key', key: e.key, shift: e.shiftKey, ctrl: e.ctrlKey });
}

// ---- touch, mouse, pinch -----------------------------------------------------------------------

const pointers = new Map();
let gesture = null;

function currentZoom(reader) {
  const z = parseFloat(getComputedStyle(reader).getPropertyValue('--zoom'));
  return Number.isFinite(z) && z > 0 ? z : 1;
}

function onPointerDown(e) {
  const stage = e.currentTarget;
  if (e.target.closest('button, a, input, select, .reader-fail, .reader-end')) return;
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  if (pointers.size === 1) {
    gesture = { id: e.pointerId, x: e.clientX, y: e.clientY, t: performance.now(), moved: false, pinch: false };
  } else if (pointers.size === 2) {
    const [a, b] = [...pointers.values()];
    const reader = document.getElementById('reader');
    gesture = { pinch: true, d0: Math.hypot(a.x - b.x, a.y - b.y), z0: currentZoom(reader), z: currentZoom(reader), moved: true };
    stage.style.touchAction = 'none';
  }
}

function onPointerMove(e) {
  const p = pointers.get(e.pointerId);
  if (!p) return;
  p.x = e.clientX;
  p.y = e.clientY;
  if (gesture) {
    gesture.lastX = e.clientX;
    gesture.lastY = e.clientY;
  }
  if (!gesture) return;
  if (gesture.pinch && pointers.size >= 2) {
    const [a, b] = [...pointers.values()];
    const d = Math.hypot(a.x - b.x, a.y - b.y);
    const z = Math.min(4, Math.max(0.5, (gesture.z0 * d) / gesture.d0));
    gesture.z = z;
    const reader = document.getElementById('reader');
    reader?.style.setProperty('--zoom', String(z));
    e.preventDefault();
  } else if (!gesture.pinch && Math.hypot(e.clientX - gesture.x, e.clientY - gesture.y) > 10) {
    gesture.moved = true;
  }
}

function onPointerUp(e) {
  const stage = e.currentTarget;
  const had = pointers.delete(e.pointerId);
  if (!had || !gesture) return;
  if (gesture.pinch) {
    if (pointers.size < 2) {
      emit({ t: 'zoom', pct: Math.round(gesture.z * 100) });
      stage.style.touchAction = '';
      gesture = pointers.size ? { ...gesture, pinch: false, moved: true, id: -1 } : null;
    }
    return;
  }
  const g = gesture;
  gesture = null;
  // The browser may take a sideways drag for a pan and cancel the pointer: the
  // last place it was seen is where the finger left.
  const cancelled = e.type === 'pointercancel';
  const endX = cancelled ? g.lastX ?? g.x : e.clientX;
  const endY = cancelled ? g.lastY ?? g.y : e.clientY;
  if (cancelled && !g.moved) return;
  const dx = endX - g.x;
  const dy = endY - g.y;
  const dt = performance.now() - g.t;
  const rect = stage.getBoundingClientRect();
  if (!g.moved && dt < 600) {
    emit({ t: 'tap', x: e.clientX - rect.left, y: e.clientY - rect.top, w: rect.width, h: rect.height });
  } else if (g.x - rect.left <= 28 && dx > 70 && dx > Math.abs(dy) * 1.5 && dt < 900) {
    // A swipe in from the left edge leaves the book, like the back gesture of a phone.
    emit({ t: 'edge_back' });
  } else if (Math.abs(dx) > 48 && Math.abs(dx) > Math.abs(dy) * 1.6 && dt < 900) {
    // Only turns the page when the page is not zoomed past the window.
    const zoomed = stage.scrollWidth > stage.clientWidth + 4;
    if (!zoomed) emit({ t: 'swipe', dx, dy });
  }
}

function onWheel(e) {
  if (!e.ctrlKey) return;
  e.preventDefault();
  emit({ t: 'wheel', dy: e.deltaY });
}

// ---- where the reader is (scroll mode) --------------------------------------------------------

let posTimer = 0;
let lastPos = '';
let lastTop = 0;
let lastUp = null;
let quietUntil = 0;
const quiet = (ms = 700) => {
  quietUntil = performance.now() + ms;
};

function reportPosition(stage) {
  posTimer = 0;
  const reader = document.getElementById('reader');
  if (!reader || reader.dataset.mode !== 'scroll' || !stage || !stage.isConnected) return;
  const top = stage.getBoundingClientRect().top;
  let hit = null;
  for (const tile of stage.querySelectorAll('.reader-tile')) {
    const r = tile.getBoundingClientRect();
    if (r.bottom > top + 8) {
      hit = tile;
      break;
    }
  }
  if (!hit) return;
  const section = hit.closest('[data-chapter]');
  if (!section) return;
  const hr = hit.getBoundingClientRect();
  const off = Math.min(0.999, Math.max(0, (top - hr.top) / Math.max(1, hr.height)));
  const nearEnd = stage.scrollHeight - stage.scrollTop - stage.clientHeight < stage.clientHeight * 2;
  const pos = { t: 'pos', chapter: section.dataset.chapter, page: Number(hit.dataset.page), off: Math.round(off * 1000) / 1000, nearEnd };
  const sig = JSON.stringify(pos);
  if (sig !== lastPos) {
    lastPos = sig;
    emit(pos);
  }
}

function onScroll(e) {
  const stage = e.currentTarget;
  // Scrolling down gets the bars out of the way, scrolling up brings them back.
  const top = stage.scrollTop;
  const delta = top - lastTop;
  if (performance.now() >= quietUntil && Math.abs(delta) >= 6) {
    const up = delta < 0 || top < 24;
    if (up !== lastUp) {
      lastUp = up;
      emit({ t: 'scroll', up });
    }
    lastTop = top;
  } else if (Math.abs(delta) >= 6) {
    lastTop = top;
  }
  if (!posTimer) posTimer = setTimeout(() => reportPosition(stage), 150);
}

// ---- commands from the UI ---------------------------------------------------------------------------

export const commands = {
  /** The reader opened (its address is /read/..., so the Back button leaves it). */
  opened() {
    lastTop = 0;
    lastUp = null;
    quiet(1000);
  },
  closed() {},
  scrollPages(n) {
    const s = document.querySelector('.reader-stage');
    s?.scrollBy({ top: n * s.clientHeight * 0.9, behavior: 'smooth' });
  },
  scrollLines(n) {
    document.querySelector('.reader-stage')?.scrollBy({ top: n * 60 });
  },
  /** Put page `index` at the top of the strip (retrying while pages above settle). */
  scrollToPage(index, frac) {
    quiet(1000);
    let tries = 0;
    const go = () => {
      const stage = document.querySelector('.reader-stage');
      const tile = stage?.querySelector(`.reader-tile[data-page="${index}"]`);
      if (tile) {
        stage.scrollTop += tile.getBoundingClientRect().top - stage.getBoundingClientRect().top;
        if (frac) stage.scrollTop += tile.getBoundingClientRect().height * frac;
      }
      if (++tries < 6) setTimeout(go, 120);
    };
    requestAnimationFrame(go);
  },
  scrollTop() {
    quiet();
    document.querySelector('.reader-stage')?.scrollTo({ top: 0 });
  },
  pin,
  fullscreen() {
    const el = document.documentElement;
    if (document.fullscreenElement) document.exitFullscreen?.();
    else el.requestFullscreen?.().catch(() => {});
  },
};

export function installReader(emitter) {
  emit = emitter;
  window.addEventListener('keydown', onKey);
  // Anything the visitor does by hand may show or hide the bars: the next scroll decides afresh.
  window.addEventListener('pointerdown', () => { lastUp = null; }, true);
  const mo = new MutationObserver((records) => {
    for (const r of records) {
      for (const n of r.removedNodes) unscan(n);
      for (const n of r.addedNodes) {
        scan(n);
        scanCovers(n);
        wireStage(n);
      }
      if (r.type === 'attributes' && r.attributeName === 'data-cover') fillCover(r.target);
    }
  });
  mo.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-cover'] });
  scan(document.body);
  scanCovers(document.body);
  wireStage(document.body);
}

function wireStage(node) {
  if (node.nodeType !== 1) return;
  const stages = node.matches?.('.reader-stage') ? [node] : node.querySelectorAll ? [...node.querySelectorAll('.reader-stage')] : [];
  for (const s of stages) {
    if (s.dataset.wired) continue;
    s.dataset.wired = '1';
    s.addEventListener('pointerdown', onPointerDown);
    s.addEventListener('pointermove', onPointerMove);
    s.addEventListener('pointerup', onPointerUp);
    s.addEventListener('pointercancel', onPointerUp);
    s.addEventListener('wheel', onWheel, { passive: false });
    s.addEventListener('scroll', onScroll, { passive: true });
  }
}
