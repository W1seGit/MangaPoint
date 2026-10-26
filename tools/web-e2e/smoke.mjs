// Opens the site and takes a picture. `node tools/web-e2e/smoke.mjs [viewport]`
import { launch, startServer, killBrowser, sleep } from './lib.mjs';

const server = await startServer();
const page = await launch({ headless: true, url: 'about:blank' });
try {
  await page.viewport(process.argv[2] || 'desktop');
  await page.goto(server.url);
  await page.waitFor("!!document.querySelector('.nav') || !!document.querySelector('.splash')", 30, 'app start');
  await sleep(800);
  console.log('shot', await page.shot(`smoke-${process.argv[2] || 'desktop'}`));
  console.log('errors', JSON.stringify(page.errors));
  console.log('text', (await page.eval('document.body.innerText')).slice(0, 300));
} finally {
  killBrowser(page);
  server.stop();
}
