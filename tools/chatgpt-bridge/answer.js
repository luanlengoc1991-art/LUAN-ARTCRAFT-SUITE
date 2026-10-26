// Reads ChatGPT's answer in the tab: waits until it is finished, then takes every image it gave,
// in order: the files it made (Python, "Download file") or else the images it drew.
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const lib = require('./lib');
const page = require('./page');
const { Tab } = require('./chrome');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Waits until the answer stops changing; its last state. */
async function waitAnswer(tab, { timeoutMs = 10 * 60 * 1000 } = {}) {
  const end = Date.now() + timeoutMs;
  let lastKey = '';
  let stableSince = Date.now();
  for (;;) {
    let s = null;
    try { s = await tab.eval(page.STATE); } catch { /* navigating to the new chat's URL */ }
    const key = s ? `${s.working}|${s.srcs.join(',')}|${s.files}|${s.text.length}` : '';
    if (key !== lastKey) { lastKey = key; stableSince = Date.now(); }
    const quiet = Date.now() - stableSince;
    // Done when nothing has changed for a while: soon after images or files, longer without
    // (ChatGPT can stop talking before its image tool has started).
    if (s && s.answered && !s.working && quiet > (s.srcs.length || s.files ? 6000 : 25000)) return s;
    if (Date.now() > end) {
      if (s && (s.srcs.length || s.files)) return s;
      throw Object.assign(new Error('ChatGPT took more than 10 minutes'), { code: 'timeout' });
    }
    await sleep(1000);
  }
}

const mimeOf = (name) => (/\.webp$/i.test(name) ? 'image/webp' : /\.jpe?g$/i.test(name) ? 'image/jpeg' : 'image/png');

/** Downloads every file of the answer into `dir` (Chrome's own download, as a click would). */
async function downloadFiles(port, tab, dir) {
  // Chrome takes the folder only as an existing, absolute Windows path; otherwise it saves to
  // the user's Downloads folder.
  dir = path.resolve(dir);
  fs.mkdirSync(dir, { recursive: true });
  const browser = await Tab.browser(port);
  try {
    // Finished downloads in arrival order: 'completed' or 'canceled'.
    const finished = [];
    browser.listeners.push((method, p) => {
      if (method === 'Browser.downloadProgress' && (p.state === 'completed' || p.state === 'canceled')) finished.push(p.state);
    });
    await browser.send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: dir, eventsEnabled: true });
    if (await tab.eval(page.EXPAND_FILES)) await sleep(1200);
    // One at a time: pressing the next button while one is saving cancels it.
    const count = await tab.eval(page.DOWNLOAD_COUNT);
    for (let n = 0; n < count; n++) {
      for (let attempt = 0; attempt < 3; attempt++) {
        const before = finished.length;
        if (!(await tab.eval(page.clickDownload(n)))) break;
        const end = Date.now() + 90000;
        while (finished.length === before && Date.now() < end) await sleep(250);
        if (finished[before] === 'completed') break;
        await sleep(1500);
      }
    }
    await browser.send('Browser.setDownloadBehavior', { behavior: 'default' }).catch(() => {});
    const names = lib.imageFiles(fs.readdirSync(dir).filter((f) => !f.endsWith('.crdownload')));
    return names.map((name) => ({ name, mime: mimeOf(name), image: fs.readFileSync(path.join(dir, name)).toString('base64') }));
  } finally {
    browser.close();
  }
}

/** Reads one image of the page: inside it first (chatgpt.com's links need the sign-in). */
async function readImage(tab, src) {
  try {
    const m = /^data:([^;,]+)?(;base64)?,(.*)$/s.exec(await tab.eval(page.readImage(src)));
    if (m) return { image: m[2] ? m[3] : Buffer.from(decodeURIComponent(m[3])).toString('base64'), mime: m[1] || 'image/png' };
  } catch { /* cross-origin: fetched from here below (signed links) */ }
  const r = await fetch(src);
  if (!r.ok) throw new Error(`downloading the image: HTTP ${r.status}`);
  return { image: Buffer.from(await r.arrayBuffer()).toString('base64'), mime: r.headers.get('content-type') || 'image/png' };
}

/** Every image of the finished answer `s`, in order, each once. */
async function collect(port, tab, s, dir) {
  let images = [];
  if (s.files) images = await downloadFiles(port, tab, dir);
  if (!images.length) {
    for (const [i, src] of s.srcs.entries()) {
      const got = await readImage(tab, src);
      images.push({ name: `ChatGPT ${String(i + 1).padStart(2, '0')}.${got.mime.includes('webp') ? 'webp' : got.mime.includes('jpeg') ? 'jpg' : 'png'}`, ...got });
    }
  }
  const seen = new Set();
  return images.filter((im) => {
    const h = crypto.createHash('sha1').update(im.image).digest('hex');
    if (seen.has(h)) return false;
    seen.add(h);
    return true;
  });
}

module.exports = { waitAnswer, downloadFiles, readImage, collect };
