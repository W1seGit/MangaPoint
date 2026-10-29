// The home page's few moving parts. Everything works without this file: the download buttons then go to
// the release page on GitHub.

// The bar at the top gets a line under it once the page has scrolled.
const bar = document.querySelector('.top');
const onScroll = () => bar.classList.toggle('scrolled', window.scrollY > 8);
window.addEventListener('scroll', onScroll, { passive: true });
requestAnimationFrame(onScroll);

// Which system this is, as far as the browser says: 'windows', 'mac', 'linux', 'phone' or ''.
function system() {
  const ua = navigator.userAgent;
  const platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
  if (/Android|iPhone|iPad|iPod/i.test(ua) || (/Mac/.test(platform) && navigator.maxTouchPoints > 1)) return 'phone';
  if (/Win/i.test(platform)) return 'windows';
  if (/Mac/i.test(platform)) return 'mac';
  if (/Linux|X11|CrOS/i.test(platform)) return 'linux';
  return '';
}

const os = system();
const get = document.getElementById('get');
const openWeb = document.getElementById('open-web');
const label = { windows: 'Download for Windows', mac: 'Download for macOS', linux: 'Download for Linux' };
if (os === 'phone') {
  // The app is for computers; on a phone the website is what can be used now.
  get.parentNode.insertBefore(openWeb, get);
  openWeb.classList.add('primary');
  get.classList.remove('primary');
  get.querySelector('span').textContent = 'Get it for your computer';
} else if (label[os]) {
  get.querySelector('span').textContent = label[os];
  document.querySelector(`.os[data-os="${os}"]`)?.classList.add('yours');
}

// GitHub's answers are kept for ten minutes in this tab, so moving round the page does not ask again (GitHub
// allows a visitor 60 questions an hour).
function fromGitHub(path) {
  const key = `gh:${path}`;
  try {
    const kept = JSON.parse(sessionStorage.getItem(key) || 'null');
    if (kept && Date.now() - kept.at < 10 * 60 * 1000) return Promise.resolve(kept.value);
  } catch {}
  return fetch(`https://api.github.com/repos/W1seGit/MangaPoint${path}`, { headers: { Accept: 'application/vnd.github+json' } })
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then((value) => {
      try { sessionStorage.setItem(key, JSON.stringify({ at: Date.now(), value })); } catch {}
      return value;
    });
}

// How many stars the repository has, just before the GitHub mark at the top (1234 is shown as 1.2k).
fromGitHub('')
  .then((repo) => {
    const n = repo.stargazers_count;
    if (typeof n !== 'number') return;
    const text = n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1).replace(/\.0$/, '')}k` : String(n);
    const count = document.getElementById('star-count');
    count.textContent = text;
    count.hidden = false;
    document.querySelector('.top-links .gh').setAttribute('aria-label', `MangaPoint on GitHub, ${n} ${n === 1 ? 'star' : 'stars'}`);
  })
  .catch(() => {});

// The latest release's files, so each button downloads its file straight away. The names carry the version
// (MangaPoint-2.0.0-windows-x86_64-setup.exe), so they are matched by how they end.
fromGitHub('/releases/latest')
  .then((release) => {
    const assets = release.assets || [];
    for (const button of document.querySelectorAll('[data-asset]')) {
      const end = '-' + button.dataset.asset;
      const file = assets.find((a) => a.name.endsWith(end));
      if (file) button.href = file.browser_download_url;
    }
    const version = (release.tag_name || '').replace(/^v/, '');
    if (version) {
      const line = document.getElementById('version');
      line.textContent = `Version ${version}, free. `;
      const notes = document.createElement('a');
      notes.href = release.html_url;
      notes.textContent = "See what's new";
      line.append(notes);
    }
    // On Windows the hero button downloads the installer itself.
    if (os === 'windows') {
      const setup = document.querySelector('[data-asset="windows-x86_64-setup.exe"]');
      if (setup && setup.href.includes('/download/')) get.href = setup.href;
    }
  })
  .catch(() => {});

// Rullo: pointed at, he is delighted and his name follows the pointer just above it, in whole steps of three
// pixels like the pixel font. A tap on a touch screen shows both for a moment.
const rullo = document.getElementById('rullo');
const nameTag = document.getElementById('name-tag');
const snap = (v) => Math.round(v / 3) * 3;
function follow(e) {
  nameTag.style.left = `${snap(e.clientX)}px`;
  nameTag.style.top = `${snap(e.clientY)}px`;
}
function greet(on) {
  rullo.classList.toggle('glad', on);
  nameTag.classList.toggle('on', on);
}
let tapTimer = 0;
rullo.addEventListener('pointerenter', (e) => { if (e.pointerType === 'mouse') { follow(e); greet(true); } });
rullo.addEventListener('pointermove', (e) => { if (e.pointerType === 'mouse') follow(e); });
rullo.addEventListener('pointerleave', (e) => { if (e.pointerType === 'mouse') greet(false); });
rullo.addEventListener('pointerdown', (e) => {
  if (e.pointerType === 'mouse') return;
  follow(e);
  greet(true);
  clearTimeout(tapTimer);
  tapTimer = setTimeout(() => greet(false), 1600);
});
// A scroll moves the page under a still pointer; the name goes until the pointer moves again.
window.addEventListener('scroll', () => { if (!rullo.matches(':hover')) greet(false); }, { passive: true });
