// Structural guards for the shared UI/formats boundary. No build or browser needed.
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = p => readFileSync(join(root, p), 'utf8');
const failures = [];
function files(folder, suffix) {
  return readdirSync(join(root, folder), { withFileTypes: true }).flatMap(e => {
    const p = `${folder}/${e.name}`;
    return e.isDirectory() ? files(p, suffix) : p.endsWith(suffix) ? [p] : [];
  });
}
const components = app => {
  const found = new Map();
  for (const p of files(`apps/${app}/src`, '.rs')) {
    const source = read(p);
    for (const m of source.matchAll(/#\[component\]\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)/g)) found.set(m[1], p);
    if (/#\[path\s*=\s*"[^"\n]*(?:desktop|pipeline)/.test(source)) failures.push(`${p}: cross-app path include`);
  }
  return found;
};
const desktop = components('desktop'), web = components('web');
for (const [name, p] of desktop) if (web.has(name)) failures.push(`${name} defined in ${p} and ${web.get(name)}`);
for (const dir of ['crates/ui/css', 'web/css']) {
  const order = read(`${dir}/ORDER`).split(/\r?\n/).map(s => s.trim()).filter(Boolean);
  const onDisk = readdirSync(join(root, dir)).filter(n => n.endsWith('.css')).sort();
  if (new Set(order).size !== order.length || JSON.stringify([...order].sort()) !== JSON.stringify(onDisk)) failures.push(`${dir}/ORDER must list every stylesheet exactly once`);
}
for (const name of ['export-split', 'preview-editor', 'zoom-scrub', 'tooltip']) {
  if (!existsSync(join(root, `crates/ui/js/${name}.js`))) failures.push(`Missing shared ${name}.js`);
  for (const path of [`web/js/${name}.js`, `apps/desktop/src/js/${name}.js`]) if (existsSync(join(root, path))) failures.push(`Copied script remains: ${path}`);
}
for (const name of ['cbz', 'epub', 'pdf', 'original']) {
  if (existsSync(join(root, `crates/web-engine/src/export/${name}.rs`))) failures.push(`Copied writer remains: ${name}`);
}
if (!existsSync(join(root, 'docs/architecture.md'))) failures.push('Missing architecture guide');

// Parse rule boundaries while preserving comments and strings. Media overrides are intentional;
// other shared selectors need a local explanation of the browser requirement.
function cssRules(css, responsive = false) {
  const result = [];
  const tokens = css.match(/\/\*[\s\S]*?\*\/|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^{}"'/]+|[{}]|./g) || [];
  let header = '', body = '', depth = 0;
  for (const t of tokens) {
    if (t === '{') { if (depth++) body += t; }
    else if (t === '}') {
      if (--depth) body += t;
      else {
        const selector = header.replace(/\/\*[\s\S]*?\*\//g, '').trim();
        if (/^@media\b/.test(selector)) result.push(...cssRules(body, true));
        else if (!selector.startsWith('@')) result.push({ selector, responsive, comment: header.includes('/*') });
        header = ''; body = '';
      }
    } else if (depth) body += t;
    else header += t;
  }
  return result;
}
function selectors(text) {
  const result = []; let depth = 0, current = '';
  for (const c of text) {
    if (c === '(' || c === '[') depth++;
    if (c === ')' || c === ']') depth--;
    if (c === ',' && depth === 0) { result.push(current.trim()); current = ''; }
    else current += c;
  }
  result.push(current.trim()); return result;
}
const sharedSelectors = new Set(files('crates/ui/css', '.css').flatMap(p => cssRules(read(p)).flatMap(r => selectors(r.selector))));
for (const p of files('web/css', '.css')) {
  for (const rule of cssRules(read(p))) {
    if (rule.responsive || rule.comment) continue;
    for (const selector of selectors(rule.selector)) {
      if (sharedSelectors.has(selector)) failures.push(`${p}: explain browser override of ${selector} in a comment`);
    }
  }
}
if (failures.length) { for (const f of failures) console.error(f); process.exitCode = 1; }
else console.log('architecture checks passed: component boundaries, shared scripts/writers, CSS order and docs');
