// Fills in the service worker: lists every file of the built site, hashes them,
// and writes web/dist/sw.js (the template web/sw.js with __VERSION__ and
// __PRECACHE__ replaced) and web/dist/precache.json.
//
//     node tools/web-precache.mjs [web/dist]
//
// The version is a hash of every file's path and content, so any change to the
// site gives a new cache name, and an unchanged site gives the same one.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dist = resolve(process.argv[2] || join(root, 'web', 'dist'));

// Files that are not part of the app shell.
const SKIP = new Set(['sw.js', 'precache.json']);

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

const files = walk(dist)
  .map((full) => relative(dist, full).split(sep).join('/'))
  .filter((p) => !SKIP.has(p) && !p.endsWith('.map'));

const entries = files.map((path) => {
  const bytes = readFileSync(join(dist, path));
  return { path, size: bytes.length, hash: createHash('sha256').update(bytes).digest('hex').slice(0, 16) };
});

const version = createHash('sha256')
  .update(entries.map((e) => `${e.path}:${e.hash}`).join('\n'))
  .digest('hex')
  .slice(0, 12);

// "index.html" doubles as the address of the app itself.
const precache = entries.map((e) => e.path);
const template = readFileSync(join(root, 'web', 'sw.js'), 'utf8');
if (!template.includes("'__VERSION__'") || !template.includes('__PRECACHE__')) throw new Error('web/sw.js lost its placeholders');
writeFileSync(join(dist, 'sw.js'), template.replace("'__VERSION__'", JSON.stringify(version)).replace('__PRECACHE__', JSON.stringify(precache, null, 2)));
writeFileSync(join(dist, 'precache.json'), JSON.stringify({ version, files: entries }, null, 2));

const total = entries.reduce((n, e) => n + e.size, 0);
console.log(`service worker: version ${version}, ${entries.length} files, ${(total / 1048576).toFixed(1)} MB`);
