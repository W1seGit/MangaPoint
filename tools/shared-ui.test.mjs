// Exercise the shared editor's host callback, rather than only checking script syntax.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

test('preview editor reports visible pages to its host after opening and scrolling', () => {
  const frames = [], listeners = {}, messages = [];
  let offset = 0;
  const rows = [0, 1, 2].map(i => ({ dataset: { row: String(i) },
    getBoundingClientRect: () => ({ top: i * 300 - offset, bottom: (i + 1) * 300 - offset }) }));
  const page = { scrollTop: 0, clientHeight: 600, getBoundingClientRect: () => ({ top: 0 }) };
  const scroller = {
    clientHeight: 300, getBoundingClientRect: () => ({ top: 0, bottom: 300, height: 300 }),
    querySelectorAll: () => rows, addEventListener: (name, cb) => { listeners[name] = cb; },
  };
  const root = { dataset: {}, isConnected: true, style: { setProperty() {} },
    closest: () => page, addEventListener() {},
    querySelector: selector => selector === '.editor-scroll' ? scroller : null,
  };
  const context = vm.createContext({
    window: { addEventListener() {} },
    document: { querySelectorAll: () => [root], documentElement: page },
    getComputedStyle: () => ({ paddingTop: '0', paddingBottom: '0' }),
    requestAnimationFrame: cb => { frames.push(cb); return frames.length; },
    ResizeObserver: class { observe() {} }, MutationObserver: class { observe() {} },
    setInterval: () => 1, clearInterval() {},
  });
  vm.runInContext(readFileSync(new URL('../crates/ui/js/preview-editor.js', import.meta.url), 'utf8'), context);
  context.window.MP.preview_editor.start(message => messages.push(Array.from(message)));
  frames.shift()();
  assert.deepEqual(messages, [[0, 1, 0]]);
  offset = 600;
  listeners.scroll(); frames.shift()();
  assert.deepEqual(messages, [[0, 1, 0], [1, 2, 2]]);
  listeners.scroll(); frames.shift()();
  assert.equal(messages.length, 2, 'unchanged positions do not resend host events');
});
