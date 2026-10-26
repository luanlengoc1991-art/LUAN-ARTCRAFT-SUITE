// ArtCraft ChatGPT: chatgpt.com in a window, signed in once with the user's own ChatGPT account,
// and a loopback server (127.0.0.1, random port, token) PhotoCraft sends image requests to.
//
// A request is run in that window the way a person would: a new chat, the picture attached, the
// request typed and sent; the image ChatGPT makes is handed back to PhotoCraft. Requests use the
// ChatGPT plan's usage. Nothing here reads or stores a password or an API key: the sign-in
// stays in the window's own browser profile (partition "persist:chatgpt").
//
// How PhotoCraft finds it: %APPDATA%\ArtCraft\chatgpt-bridge.json = {port, token, pid, version}.
//   GET  /v1/status                      -> {ok, loggedIn, busy, version}
//   POST /v1/image {prompt, image?: b64}  -> {ok, image: b64, mime, text} | {ok:false, code, error}
// Every request needs `Authorization: Bearer <token>`.
'use strict';

const { app, BrowserWindow, Menu, shell } = require('electron');
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const os = require('os');
const path = require('path');
const lib = require('./lib');

const HOME = 'https://chatgpt.com/';
const VERSION = require('./package.json').version;
const AUTH_HOSTS = ['accounts.google.com', 'login.microsoftonline.com', 'login.live.com', 'appleid.apple.com', 'auth.openai.com', 'auth0.openai.com', 'chatgpt.com'];

let win = null;
let server = null;
let token = '';
let busy = false;
let queue = Promise.resolve();

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => showWindow(true));
  app.whenReady().then(start);
}

app.userAgentFallback = lib.chromeUserAgent(process.versions.chrome);

function start() {
  Menu.setApplicationMenu(Menu.buildFromTemplate([
    {
      label: 'ArtCraft ChatGPT',
      submenu: [
        { label: 'Trang chủ ChatGPT', click: () => win && win.loadURL(HOME) },
        { label: 'Tải lại', accelerator: 'F5', click: () => win && win.reload() },
        { label: 'Công cụ nhà phát triển', accelerator: 'F12', click: () => win && win.webContents.toggleDevTools() },
        { type: 'separator' },
        { label: 'Thoát', accelerator: 'Ctrl+Q', click: () => app.quit() },
      ],
    },
  ]));
  win = new BrowserWindow({
    width: 1100,
    height: 820,
    title: 'ArtCraft ChatGPT',
    webPreferences: { partition: 'persist:chatgpt', contextIsolation: true, sandbox: true, backgroundThrottling: false },
  });
  win.on('page-title-updated', (e) => e.preventDefault());
  // Sign-in pages may open in a popup; everything else opens in the normal browser.
  win.webContents.setWindowOpenHandler(({ url }) => {
    let host = '';
    try { host = new URL(url).hostname; } catch { /* not a URL */ }
    if (AUTH_HOSTS.some((h) => host === h || host.endsWith(`.${h}`))) {
      return { action: 'allow', overrideBrowserWindowOptions: { width: 520, height: 700, webPreferences: { partition: 'persist:chatgpt' } } };
    }
    if (/^https?:/.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  win.on('closed', () => { win = null; app.quit(); });
  win.loadURL(HOME);
  listen();
}

function setStatus(text) {
  if (win) win.setTitle(text ? `ArtCraft ChatGPT: ${text}` : 'ArtCraft ChatGPT');
}

function showWindow(focus) {
  if (!win) return;
  if (win.isMinimized()) win.restore();
  if (focus) { win.show(); win.focus(); } else if (!win.isVisible()) win.showInactive();
}

// ------------------------------------------------------------------------------------ Server

function listen() {
  token = crypto.randomBytes(32).toString('hex');
  server = http.createServer(handle);
  server.listen(0, '127.0.0.1', () => {
    const { port } = server.address();
    const file = lib.infoPath();
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ port, token, pid: process.pid, version: VERSION }));
  });
}

app.on('will-quit', () => {
  try {
    const file = lib.infoPath();
    const info = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (info.pid === process.pid) fs.unlinkSync(file);
  } catch { /* already gone */ }
});

function reply(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text), Connection: 'close' });
  res.end(text);
}

function handle(req, res) {
  // Only loopback callers that know the token; a web page can't read the token file.
  if (!lib.authorized(req.headers, token)) return reply(res, 401, { ok: false, code: 'auth', error: 'bad token' });
  if (req.method === 'GET' && req.url === '/v1/status') {
    loggedIn().then((ok) => reply(res, 200, { ok: true, loggedIn: ok, busy, version: VERSION }), (e) => reply(res, 200, { ok: true, loggedIn: false, busy, version: VERSION, error: String(e) }));
    return;
  }
  if (req.method === 'POST' && req.url === '/v1/image') {
    const chunks = [];
    let size = 0;
    req.on('data', (c) => {
      size += c.length;
      if (size > 80 * 1024 * 1024) { reply(res, 413, { ok: false, code: 'size', error: 'request too large' }); req.destroy(); return; }
      chunks.push(c);
    });
    req.on('end', () => {
      let body;
      try { body = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { return reply(res, 400, { ok: false, code: 'json', error: 'invalid JSON' }); }
      if (!body || typeof body.prompt !== 'string' || !body.prompt.trim()) return reply(res, 400, { ok: false, code: 'prompt', error: 'missing prompt' });
      // One job at a time: they share the one window.
      const job = queue.then(() => runJob(body));
      queue = job.catch(() => {});
      job.then((r) => reply(res, 200, r), (e) => reply(res, 200, { ok: false, code: e.code || 'error', error: e.message || String(e) }));
    });
    return;
  }
  reply(res, 404, { ok: false, code: 'route', error: 'not found' });
}

// ------------------------------------------------------------------------------- The page

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function js(code) {
  return win.webContents.executeJavaScript(code, true);
}

async function waitFor(code, timeoutMs, what) {
  const end = Date.now() + timeoutMs;
  for (;;) {
    try { if (await js(code)) return; } catch { /* page navigating */ }
    if (Date.now() > end) throw Object.assign(new Error(`timed out waiting for ${what}`), { code: 'timeout' });
    await sleep(400);
  }
}

async function loggedIn() {
  if (!win) return false;
  const url = win.webContents.getURL();
  if (!url.startsWith(HOME)) return false;
  return js(`fetch('/api/auth/session', {credentials: 'include'}).then(r => r.ok ? r.json() : {}).then(s => !!(s && s.user)).catch(() => false)`);
}

function cdp(method, params) {
  const d = win.webContents.debugger;
  if (!d.isAttached()) d.attach('1.3');
  return d.sendCommand(method, params || {});
}

// ChatGPT's composer has had several forms (ProseMirror #prompt-textarea, a plain textarea).
const COMPOSER = `document.querySelector('#prompt-textarea, #mobile-composer-prompt, div[contenteditable="true"].ProseMirror, form textarea')`;

async function attach(file) {
  const { result } = await cdp('Runtime.evaluate', {
    expression: `(() => {
      const all = [...document.querySelectorAll('input[type=file]')];
      return document.querySelector('#upload-photos')
        || all.find(i => (i.accept || '').includes('image') && !i.hasAttribute('capture'))
        || all.find(i => !i.hasAttribute('capture')) || all[0] || null;
    })()`,
  });
  if (!result || !result.objectId) throw Object.assign(new Error('ChatGPT page has no file upload (are you signed in?)'), { code: 'page' });
  await cdp('DOM.setFileInputFiles', { files: [file], objectId: result.objectId });
  // The picture shows in the composer before the upload ends; Send stays disabled until it ends.
  await waitFor(`(() => { const c = ${COMPOSER}; const f = c && (c.closest('form') || c.parentElement.parentElement.parentElement); return !!(f && f.querySelector('img')); })()`, 60000, 'the attached picture');
}

async function type(text) {
  await js(`(() => { const c = ${COMPOSER}; if (!c) return false; c.focus(); return true; })()`);
  await cdp('Input.insertText', { text });
}

const SEND = `(() => {
  const c = ${COMPOSER};
  const f = c && c.closest('form');
  return document.querySelector('[data-testid="send-button"], #composer-submit-button') || (f && f.querySelector('button[type=submit]')) || document.querySelector('button[aria-label^="Send"]');
})()`;

/** The page's state after the last request: the images ChatGPT gave (largest last), whether it's still working, its words. */
// Messages are marked data-message-author-role (older page) or li[data-message-role] (newer).
const USER = `[data-message-author-role="user"], [data-message-role="user"]`;
const STATE = `(() => {
  const users = document.querySelectorAll('${USER}');
  const last = users[users.length - 1];
  if (!last) return null;
  const after = (n) => !!(last.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING) && !last.contains(n);
  const blurred = (n) => { for (let e = n, i = 0; e && i < 5; e = e.parentElement, i++) { const f = getComputedStyle(e).filter || ''; if (f.includes('blur')) return true; } return false; };
  // Not the user's own picture (its turn holds the user message) nor the composer's preview.
  const theirs = (i) => { const t = i.closest('article, li, [data-testid^="conversation-turn"]'); return !i.closest('form') && !(t && (t.matches('${USER}') || t.querySelector('${USER}'))); };
  const imgs = [...document.querySelectorAll('img')].filter(i => after(i) && theirs(i) && i.complete && i.naturalWidth >= 256 && i.naturalHeight >= 256);
  imgs.sort((a, b) => a.naturalWidth * a.naturalHeight - b.naturalWidth * b.naturalHeight);
  const answers = [...document.querySelectorAll('[data-message-author-role="assistant"], [data-message-role="assistant"]')].filter(after);
  // Each answer starts with a screen-reader heading ("ChatGPT said:"), in the page's language.
  const text = answers.map(a => a.innerText.trim().replace(/^[^\\n]{0,40}:\\n/, '').trim()).filter(Boolean).join('\\n').slice(0, 2000);
  const lastAnswer = answers[answers.length - 1];
  const turn = lastAnswer ? (lastAnswer.closest('article, li') || lastAnswer) : null;
  const working = !!document.querySelector('[data-testid="stop-button"], button[aria-label="Stop streaming"], button[aria-label="Stop generating"], button[aria-label^="Stop"]')
    || !!(lastAnswer && lastAnswer.matches('[data-message-role]') && !lastAnswer.hasAttribute('data-message-complete'))
    || !!(turn && turn.querySelector('[class*="shimmer"], [class*="animate-pulse"], [aria-busy="true"]'))
    || imgs.some(blurred);
  const best = imgs[imgs.length - 1];
  return { src: best ? best.currentSrc || best.src : '', count: imgs.length, working, answered: answers.length > 0, text };
})()`;

async function waitAnswer() {
  const end = Date.now() + 8 * 60 * 1000;
  let lastKey = '';
  let stableSince = Date.now();
  for (;;) {
    let s = null;
    try { s = await js(STATE); } catch { /* navigating to the new chat's URL */ }
    const key = s ? `${s.working}|${s.src}|${s.count}|${s.text.length}` : '';
    if (key !== lastKey) { lastKey = key; stableSince = Date.now(); }
    const quiet = Date.now() - stableSince;
    // Done when nothing has changed for a while: soon after an image, longer without one
    // (ChatGPT can stop talking before its image tool has started).
    if (s && s.answered && !s.working && quiet > (s.src ? 5000 : 25000)) return s;
    if (Date.now() > end) {
      if (s && s.src) return s;
      throw Object.assign(new Error('ChatGPT took more than 8 minutes'), { code: 'timeout' });
    }
    await sleep(1000);
  }
}

async function download(src) {
  if (/^https?:/.test(src)) {
    const r = await win.webContents.session.fetch(src);
    if (!r.ok) throw new Error(`downloading the image: HTTP ${r.status}`);
    return { image: Buffer.from(await r.arrayBuffer()).toString('base64'), mime: r.headers.get('content-type') || 'image/png' };
  }
  // blob: and data: images are read inside the page.
  const dataUrl = await js(`fetch(${JSON.stringify(src)}).then(r => r.blob()).then(b => new Promise((ok, no) => { const f = new FileReader(); f.onload = () => ok(f.result); f.onerror = no; f.readAsDataURL(b); }))`);
  const m = /^data:([^;,]+)?(;base64)?,(.*)$/s.exec(dataUrl || '');
  if (!m) throw new Error('reading the image failed');
  return { image: m[2] ? m[3] : Buffer.from(decodeURIComponent(m[3])).toString('base64'), mime: m[1] || 'image/png' };
}

async function runJob(body) {
  if (!win) throw Object.assign(new Error('the ArtCraft ChatGPT window is closed'), { code: 'window' });
  busy = true;
  let file = null;
  try {
    setStatus('đang làm yêu cầu từ PhotoCraft…');
    showWindow(false);
    await win.loadURL(HOME);
    await waitFor(`!!${COMPOSER}`, 60000, 'the ChatGPT page');
    if (!(await loggedIn())) {
      showWindow(true);
      throw Object.assign(new Error('Not signed in to ChatGPT: sign in in the ArtCraft ChatGPT window, then send again.'), { code: 'login' });
    }
    const hasImage = typeof body.image === 'string' && body.image.length > 0;
    if (hasImage) {
      file = path.join(os.tmpdir(), `artcraft-chatgpt-${process.pid}-${Date.now()}.png`);
      fs.writeFileSync(file, Buffer.from(body.image, 'base64'));
      await attach(file);
    }
    await type(lib.buildPrompt(body.prompt, hasImage));
    await waitFor(`(() => { const b = ${SEND}; return !!(b && !b.disabled && b.getAttribute('aria-disabled') !== 'true'); })()`, 180000, 'the Send button');
    await sleep(300);
    await js(`${SEND}.click()`);
    await waitFor(`document.querySelectorAll('${USER}').length > 0`, 60000, 'the request to be sent');
    const s = await waitAnswer();
    const got = s.src ? await download(s.src) : {};
    return lib.jobResult({ ...got, text: s.text });
  } finally {
    busy = false;
    setStatus('');
    if (file) fs.unlink(file, () => {});
  }
}
