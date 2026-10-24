// Keyboard shortcuts: "/" or Ctrl+K go to the search, Esc closes whatever is
// open (a menu, a dropdown, a dialog), and in the chapter table the arrow keys
// move a marker and Space ticks the chapter it is on.
if (!window.mpKeysReady) {
  window.mpKeysReady = true;

  const typing = (el) =>
    !!el &&
    ((el.tagName === 'INPUT' && !['checkbox', 'radio', 'button', 'submit'].includes(el.type)) ||
      el.tagName === 'TEXTAREA' ||
      el.tagName === 'SELECT' ||
      el.isContentEditable);

  const goSearch = () => {
    const nav = document.getElementById('nav-Search');
    if (nav && !nav.classList.contains('active')) nav.click();
    let tries = 0;
    const focus = () => {
      const box = document.querySelector('.searchbox:not(.searchbox-ghost) .searchbox-input');
      if (box && box.offsetParent !== null) {
        box.focus();
        box.select();
      } else if (tries++ < 30) {
        setTimeout(focus, 50);
      }
    };
    focus();
  };

  // What Esc closes, the topmost first. Each is closed by clicking what the
  // mouse would click, so it behaves as it does with the mouse.
  const closers = [
    '.menu-scrim',
    '.backdrop',
    '.overlay .modal .actions .btn:not(.primary):not(.danger)',
    '.overlay .as-head .btn',
    '.overlay',
  ];
  const closeOpen = () => {
    for (const selector of closers) {
      const el = document.querySelector(selector);
      if (el) {
        el.click();
        return true;
      }
    }
    return false;
  };

  const rows = () =>
    [...document.querySelectorAll('table.chapters tbody tr')].filter(
      (r) => !r.classList.contains('locked') && r.offsetParent !== null
    );
  const cursorOf = (list) => list.findIndex((r) => r.hasAttribute('data-cursor'));
  const moveCursor = (delta, edge) => {
    const list = rows();
    if (!list.length) return false;
    const at = cursorOf(list);
    let next;
    if (edge === 'start') next = 0;
    else if (edge === 'end') next = list.length - 1;
    else if (at < 0) next = delta > 0 ? 0 : list.length - 1;
    else next = Math.max(0, Math.min(list.length - 1, at + delta));
    list.forEach((r) => r.removeAttribute('data-cursor'));
    list[next].setAttribute('data-cursor', '');
    list[next].scrollIntoView({ block: 'nearest' });
    return true;
  };

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (closeOpen()) e.preventDefault();
      else if (typing(document.activeElement)) document.activeElement.blur();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      goSearch();
      return;
    }
    if (typing(document.activeElement) || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === '/') {
      e.preventDefault();
      goSearch();
    } else if (e.key === 'ArrowDown' && moveCursor(1)) {
      e.preventDefault();
    } else if (e.key === 'ArrowUp' && moveCursor(-1)) {
      e.preventDefault();
    } else if (e.key === 'Home' && moveCursor(0, 'start')) {
      e.preventDefault();
    } else if (e.key === 'End' && moveCursor(0, 'end')) {
      e.preventDefault();
    } else if (e.key === ' ') {
      const list = rows();
      const at = cursorOf(list);
      if (at >= 0) {
        e.preventDefault();
        list[at].click();
      }
    }
  });
}
