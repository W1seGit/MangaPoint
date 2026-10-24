// Each page keeps its scroll position while the app is open: going to another
// page and coming back puts the page where it was.
if (!window.mpScrollReady) {
  window.mpScrollReady = true;
  const saved = {};

  // Which page this is: the dock or corner item it belongs to, and whatever tells
  // series, settings sections and the Export screen apart.
  const where = () => {
    const nav = document.querySelector('.nav .nav-item.active');
    // (the dock shows a name only under the item that is on, so the id is the key)
    const series = document.querySelector('.overview h1, .page-head h1');
    const section = document.querySelector('.settings-nav .nav-item.active');
    return [
      nav ? nav.id : '',
      series ? series.innerText.trim() : '',
      section ? section.innerText.trim() : '',
    ].join('|');
  };
  let current = where();

  // The scrolling is the content area's. Scroll events do not bubble, so they
  // are caught on the way down.
  document.addEventListener(
    'scroll',
    (e) => {
      if (e.target && e.target.classList && e.target.classList.contains('content')) {
        saved[current] = e.target.scrollTop;
      }
    },
    true
  );

  const check = () => {
    const next = where();
    if (next === current) return;
    current = next;
    const want = saved[next] || 0;
    const apply = () => {
      const content = document.querySelector('.content');
      if (content) content.scrollTop = want;
    };
    requestAnimationFrame(apply);
    // Pages that fill in after they open (chapter lists) get another try.
    setTimeout(apply, 150);
    setTimeout(apply, 450);
  };
  // Checked as soon as the page changes, before the browser sends the scroll
  // event that the new, shorter or longer page causes.
  new MutationObserver(check).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['class'] });
}
