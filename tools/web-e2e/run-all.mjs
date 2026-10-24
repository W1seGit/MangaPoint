// Runs every browser test at the three viewports. Build and have fixtures first (see README.md).
//   node tools/web-e2e/run-all.mjs [--skip-big]
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const skipBig = process.argv.includes('--skip-big');
const plan = [];
for (const vp of ['desktop', 'tablet', 'phone']) {
  for (const t of ['import', 'read', 'chrome', 'export', 'state', 'routes', 'library']) plan.push([t, vp]);
}
if (!skipBig) plan.push(['big', 'desktop']);
let failed = 0;
for (const [t, vp] of plan) {
  console.log(`\n=== e2e-${t} ${vp}`);
  const r = spawnSync(process.execPath, [join(here, `e2e-${t}.mjs`), vp], { stdio: 'inherit' });
  if (r.status !== 0) failed++;
}
console.log(`\n${plan.length - failed}/${plan.length} suites passed`);
process.exit(failed ? 1 : 0);
