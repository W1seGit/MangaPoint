// The address bar follows the page: `/` is the library, `/export` the Export screen,
// `/settings#reader` a part of Settings and `/read?book=<id>&chapter=<n>` a book being read. Opening one
// of these addresses (or going back) shows that screen. The app lives in one page, so any host
// that serves index.html for them (the service worker does, offline) shows it.

// Where the app is served from: `/` on its own domain, `/something/` under a path.
const base = new URL('../', import.meta.url).pathname;

/** The address now, as `{ path, hash }` without the base and the slashes around it. */
export function current() {
  const rel = location.pathname.startsWith(base) ? location.pathname.slice(base.length) : '';
  return { path: rel.replace(/^\/+|\/+$/g, '') + location.search, hash: decodeURIComponent(location.hash.slice(1)) };
}

const readOf = (path) => /^read\?(?:.*&)?book=([^&]+)/.exec(path)?.[1] ?? null;

// The addresses this page put in the history, by depth, so that leaving the reader (or any
// screen) for the one before it goes back instead of adding another entry.
const trail = [];
let depth = history.state && Number.isInteger(history.state.d) ? history.state.d : 0;
trail[depth] = key(current());
history.replaceState({ d: depth }, '');

function key(r) {
  return `${r.path}#${r.hash || ''}`;
}

/** Show this address. Moving on in the same book keeps one entry in the history. */
export function go(path, hash) {
  const now = current();
  if (now.path === path && now.hash === (hash || '')) return;
  const url = base + path + (hash ? `#${encodeURIComponent(hash)}` : '');
  const wanted = key({ path, hash });
  const sameBook = readOf(now.path) !== null && readOf(now.path) === readOf(path);
  if (sameBook) {
    history.replaceState({ d: depth }, '', url);
    trail[depth] = wanted;
  } else if (readOf(now.path) !== null && depth > 0 && trail[depth - 1] === wanted) {
    // Closing the book: the entry before it is where it was opened from.
    history.back();
  } else {
    depth += 1;
    history.pushState({ d: depth }, '', url);
    trail.length = depth;
    trail[depth] = wanted;
  }
}

/** Replace the address without a new entry in the history (the screen was not found). */
export function fix(path, hash) {
  history.replaceState({ d: depth }, '', base + path + (hash ? `#${encodeURIComponent(hash)}` : ''));
  trail[depth] = key({ path, hash });
}

/** `onChange({ path, hash })` when the person goes back or forward. */
export function watch(onChange) {
  window.addEventListener('popstate', (e) => {
    depth = e.state && Number.isInteger(e.state.d) ? e.state.d : 0;
    trail[depth] = key(current());
    onChange(current());
  });
}
