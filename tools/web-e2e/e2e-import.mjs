// Importing every format, and every kind of bad file. node tools/web-e2e/e2e-import.mjs [viewport]
import { launch, startServer, killBrowser, sleep, Report } from './lib.mjs';
import { fx, openApp, importBooks, chooseFiles, waitReview, closeImport, titles, hScroll } from './helpers.mjs';

const viewport = process.argv[2] || 'desktop';
const r = new Report();
const server = await startServer();
const page = await launch({ headless: true });
try {
  await openApp(page, server.url, viewport);
  r.check('empty library says so', /library is empty/i.test(await page.eval('document.body.innerText')));
  await page.shot(`${viewport}-1-empty`);

  // ---- every supported format ----
  let text = await importBooks(page, [fx('Hero Saga - Chapter 01.cbz')]);
  r.check('CBZ: review showed chapters and Add worked', /chapter/i.test(text) && (await titles(page)).length === 1, text.slice(0, 80).replace(/\n/g, ' | '));
  await page.shot(`${viewport}-2-one-book`);

  text = await importBooks(page, [fx('Hero Saga Vol 2.zip')]);
  r.check('ZIP of chapter folders: 3 chapters found', /3 chapters/.test(text), text.replace(/\n/g, ' | ').slice(0, 120));

  text = await importBooks(page, [fx('Picture Book.epub')]);
  r.check('EPUB with pictures imported', (await titles(page)).length === 3, JSON.stringify(await titles(page)));

  text = await importBooks(page, [fx('Loose Pics', 'img_01.png'), fx('Loose Pics', 'img_02.png'), fx('Loose Pics', 'img_03.png'), fx('Loose Pics', 'img_04.png')]);
  r.check('loose pictures became one book', (await titles(page)).length === 4 && /4 pages/.test(text), text.replace(/\n/g, ' | ').slice(0, 120));

  text = await importBooks(page, [fx('Color Test.cbz'), fx('Color Webtoon.cbz')]);
  r.check('two files at once give two books', (await titles(page)).length === 6, JSON.stringify(await titles(page)));
  await page.shot(`${viewport}-3-library`);
  r.check('no sideways scrolling in the library', JSON.stringify(await hScroll(page)) === '{"doc":0,"body":0}', JSON.stringify(await hScroll(page)));

  // ---- a folder, through the real "Choose a folder" ----
  await page.click('#import-button');
  page.answerFileChooser([fx('Loose Pics')]);
  await page.click('#import-folder');
  await sleep(2500);
  const folderDialog = await page.eval("!!document.querySelector('#import-dialog') ? document.querySelector('#import-dialog').innerText : ''");
  r.check('a folder of pictures was read (10 pages)', /10 pages/.test(folderDialog), folderDialog.replace(/\n/g, ' | ').slice(0, 140));
  if (/#import-add/.test('') || (await page.rect('#import-add'))) {
    await page.click('#import-add');
    await page.waitFor("!document.querySelector('#import-dialog')", 60, 'folder import');
  } else if (await page.rect('#import-cancel')) {
    await closeImport(page);
  }
  console.log('books now:', JSON.stringify(await titles(page)));

  // ---- not supported, and bad files ----
  const bad = async (name, file, expect) => {
    await chooseFiles(page, [file]);
    await waitReview(page, 60);
    const t = await page.eval("document.querySelector('#import-dialog').innerText");
    r.check(`${name}: clear message`, expect.test(t), t.replace(/\n/g, ' | ').slice(0, 200));
    await page.shot(`${viewport}-bad-${name.replace(/\W+/g, '-')}`);
    await closeImport(page);
    r.check(`${name}: page still alive`, (await page.eval('1+1')) === 2 && !!(await page.rect('#import-button')));
  };
  await bad('7z', fx('Seven Stories.7z'), /7z.*not supported on the web yet/i);
  await bad('pdf', fx('Scanned Book.pdf'), /PDF.*not supported on the web yet/i);
  await bad('empty zip', fx('bad', 'empty.zip'), /pictures|zip|damaged|too small/i);
  await bad('garbage cbz', fx('bad', 'garbage.cbz'), /not a ZIP|damaged|cut short/i);
  await bad('not images', fx('bad', 'notimages.zip'), /No pictures|not pictures/i);
  await bad('traversal', fx('bad', 'traversal.zip'), /unsafe|No pictures|unusable/i);
  await bad('truncated', fx('bad', 'truncated.cbz'), /not a ZIP|damaged|cut short/i);
  // A huge picture: it imports, and the reader will say if it cannot show it.
  await chooseFiles(page, [fx('bad', 'hugepicture.cbz')]);
  await waitReview(page, 60);
  const huge = await page.eval("document.querySelector('#import-dialog').innerText");
  r.check('huge-picture file gets a review or a clear message', /page|picture|huge|big/i.test(huge), huge.replace(/\n/g, ' | ').slice(0, 160));
  await closeImport(page);

  // ---- the 20,000-entry zip must not hang the page ----
  const t0 = Date.now();
  await chooseFiles(page, [fx('bad', 'manyfiles.zip')]);
  await waitReview(page, 120);
  const many = await page.eval("document.querySelector('#import-dialog').innerText");
  console.log(`manyfiles: ${Date.now() - t0} ms: ${many.replace(/\n/g, ' | ').slice(0, 200)}`);
  r.check('20,000-entry zip answered without hanging', Date.now() - t0 < 100000, `${Date.now() - t0} ms`);
  await page.shot(`${viewport}-bad-manyfiles`);
  await closeImport(page);

  r.check('console free of errors', page.errors.length === 0, page.errors.slice(0, 3).join(' || '));
} finally {
  const failed = r.summary();
  killBrowser(page);
  server.stop();
  process.exit(failed ? 1 : 0);
}
