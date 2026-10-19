// Saving and loading the reading-state file (.mpstate).
//
// Where the browser has the File System Access API (Chrome, Edge) the file is
// chosen once, the handle is kept in IndexedDB, and later saves write straight
// to it after a permission prompt. Elsewhere (Firefox, Safari) the file is
// downloaded, and loading uses an ordinary file picker.
import { dbGet, dbPut, dbDelete } from './store.js';

const HANDLE_KEY = 'state-handle';
const TYPES = [{ description: 'MangaPoint reading state', accept: { 'application/json': ['.mpstate'] } }];

export const canPickSave = () => typeof window !== 'undefined' && typeof window.showSaveFilePicker === 'function';

function download(text, name) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/octet-stream' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}

async function writeTo(handle, text) {
  const w = await handle.createWritable();
  await w.write(text);
  await w.close();
}

/** The name of the file reading state is saved to, if one was chosen. */
export async function savedName() {
  try {
    const h = await dbGet('meta', HANDLE_KEY);
    return h ? h.name : null;
  } catch (_) {
    return null;
  }
}

export async function forgetHandle() {
  try {
    await dbDelete('meta', HANDLE_KEY);
  } catch (_) { /* nothing to forget */ }
}

/**
 * Save `text`. With `choose`, ask where. Resolves to { mode, name } where mode
 * is 'file' (written to the chosen file) or 'download'. Throws on a real error.
 */
export async function saveState(text, suggested, choose) {
  if (canPickSave()) {
    let handle = null;
    if (!choose) {
      try {
        handle = await dbGet('meta', HANDLE_KEY);
      } catch (_) { /* no handle */ }
      if (handle) {
        try {
          let perm = await handle.queryPermission({ mode: 'readwrite' });
          if (perm !== 'granted') perm = await handle.requestPermission({ mode: 'readwrite' });
          if (perm !== 'granted') handle = null;
        } catch (_) {
          handle = null;
        }
      }
    }
    if (!handle) {
      try {
        handle = await window.showSaveFilePicker({ suggestedName: suggested, types: TYPES });
      } catch (e) {
        if (e && e.name === 'AbortError') return { mode: 'cancelled', name: '' };
        handle = null; // blocked (no user gesture): fall back to a download
      }
    }
    if (handle) {
      try {
        await writeTo(handle, text);
        await dbPut('meta', HANDLE_KEY, handle).catch(() => {});
        return { mode: 'file', name: handle.name };
      } catch (e) {
        console.warn('could not write to the chosen file', e);
        await forgetHandle();
      }
    }
  }
  download(text, suggested);
  return { mode: 'download', name: suggested };
}

/** Ask for a file to load. Resolves to { name, text } or null if cancelled. */
export function pickStateFile() {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.mpstate,application/json';
    input.style.display = 'none';
    input.addEventListener('change', async () => {
      const f = input.files && input.files[0];
      input.remove();
      if (!f) return resolve(null);
      resolve({ name: f.name, text: await f.text() });
    });
    input.addEventListener('cancel', () => {
      input.remove();
      resolve(null);
    });
    document.body.appendChild(input);
    input.click();
  });
}
