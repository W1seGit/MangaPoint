// Starts the page: the browser glue first, then the WebAssembly (the Rust UI).
import mp from './mp.js';
import './ui/tooltip.js';
import './ui/zoom-anchor.js';

window.MP.tooltip.start();
window.MP.zoom_anchor.start();

function fail(message) {
  const boot = document.getElementById('boot') || document.getElementById('main');
  if (boot) {
    boot.innerHTML = '';
    const p = document.createElement('p');
    p.textContent = message;
    p.style.cssText = 'padding:24px;max-width:520px;margin:auto;color:#f08585';
    boot.appendChild(p);
  }
}

try {
  if (typeof WebAssembly !== 'object') throw new Error('This browser does not support WebAssembly.');
  const wasm = await import('../pkg/mangapoint_web.js');
  await wasm.default();
  mp.setEngine(wasm);
  // The page's own content has replaced the first-paint note.
  document.getElementById('boot')?.remove();
} catch (e) {
  console.error(e);
  fail(`MangaPoint could not start: ${e && e.message ? e.message : e}`);
}
