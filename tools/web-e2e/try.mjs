// Scratch driver: node tools/web-e2e/try.mjs <viewport> <file...>
import { launch, startServer, killBrowser, sleep, FIXTURES } from './lib.mjs';
import { join } from 'node:path';

const vp = process.argv[2] || 'desktop';
const files = process.argv.slice(3).map((f) => join(FIXTURES, f));
const server = await startServer();
const page = await launch({ headless: true });
try {
  await page.viewport(vp);
  await page.goto(server.url);
  await page.waitFor("!!document.querySelector('.nav')", 30, 'app');
  page.answerFileChooser(files);
  await page.click('#import-button');
  await page.click('#import-files');
  await page.waitFor("!!document.querySelector('#import-add, #import-failures')", 30, 'review');
  await sleep(600);
  console.log(await page.shot(`try-${vp}-review`));
  console.log('errors', JSON.stringify(page.errors));
  console.log((await page.eval("document.querySelector('#import-dialog').innerText")).slice(0, 800));
} finally {
  killBrowser(page);
  server.stop();
}
