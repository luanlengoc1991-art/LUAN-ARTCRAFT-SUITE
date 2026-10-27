// ArtCraft ChatGPT: chatgpt.com in the user's own Chrome, signed in once with their ChatGPT
// account, and a loopback server (127.0.0.1, random port, token) PhotoCraft sends image
// requests to. This window shows the state and has the sign-in button.
//
// Google refuses sign-in inside embedded browsers, so chatgpt.com runs in real Chrome (or
// Edge) with a profile of its own (`chrome.js`): signed in once in a normal Chrome window, then
// driven through a loopback DevTools port. A request runs there as a person would: a new chat,
// the picture attached, the request typed and sent; the image ChatGPT makes goes back to
// PhotoCraft. Requests use the ChatGPT plan's usage. Nothing here reads or stores a password
// or an API key: the sign-in stays in that Chrome profile.
//
// How PhotoCraft finds it: %APPDATA%\ArtCraft\chatgpt-bridge.json = {port, token, pid, version}.
//   GET  /v1/status                      -> {ok, loggedIn, busy, version}
//   POST /v1/image {prompt, image?: b64}  -> {ok, image: b64, mime, text} | {ok:false, code, error}
// Every request needs `Authorization: Bearer <token>`.
'use strict';

const { app, BrowserWindow, ipcMain, Menu } = require('electron');
const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const os = require('os');
const path = require('path');
const lib = require('./lib');
const chrome = require('./chrome');
const page = require('./page');
const answer = require('./answer');

// Started by PhotoCraft: work out of sight; the window shows only to sign in.
const background = process.argv.includes('--background');

const VERSION = require('./package.json').version;

let win = null;
let server = null;
let token = '';
let busy = false;
let queue = Promise.resolve();
const state = { signedIn: null, activity: 'Sẵn sàng', error: '', browser: chrome.findBrowser() };

let quitting = false;
app.on('before-quit', () => { quitting = true; log('quit'); });
// Hidden windows don't count: the app runs until "Thoát".
app.on('window-all-closed', () => {});

/** %LOCALAPPDATA%\ArtCraft ChatGPT\bridge.log: what happened, when a request goes wrong. */
const LOG = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'ArtCraft ChatGPT', 'bridge.log');
function log(line) {
  try {
    fs.mkdirSync(path.dirname(LOG), { recursive: true });
    if (fs.existsSync(LOG) && fs.statSync(LOG).size > 2 * 1024 * 1024) fs.renameSync(LOG, `${LOG}.old`);
    fs.appendFileSync(LOG, `${new Date().toISOString()} ${line}\n`);
  } catch { /* logging never stops a request */ }
}
process.on('uncaughtException', (e) => log(`uncaught: ${e && e.stack ? e.stack : e}`));
process.on('unhandledRejection', (e) => log(`unhandled: ${e && e.stack ? e.stack : e}`));

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', (_e, argv) => { if (!argv.includes('--background')) showWindow(); });
  app.whenReady().then(start);
}

function start() {
  Menu.setApplicationMenu(null);
  win = new BrowserWindow({
    width: 460,
    height: 480,
    title: 'ArtCraft ChatGPT',
    resizable: false,
    show: !background,
    webPreferences: { preload: path.join(__dirname, 'preload.js'), contextIsolation: true, sandbox: true },
  });
  win.loadFile(path.join(__dirname, 'status.html'));
  // Closing the window only hides it: PhotoCraft may be waiting for an answer. "Thoát" quits.
  win.on('close', (e) => {
    if (!quitting) {
      e.preventDefault();
      win.hide();
    }
  });
  win.on('closed', () => { win = null; });
  ipcMain.handle('state', () => state);
  ipcMain.handle('signIn', () => signIn());
  ipcMain.handle('check', () => check());
  ipcMain.handle('quit', () => app.quit());
  listen();
  check();
}

function showWindow() {
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
}

function update(patch) {
  Object.assign(state, patch);
  if (win) win.webContents.send('state', state);
}

async function signIn() {
  try {
    update({ activity: 'Đang mở Chrome để đăng nhập…', error: '' });
    await chrome.openSignIn();
    update({ signedIn: null, activity: 'Đăng nhập ChatGPT trong cửa sổ Chrome vừa mở. Xong thì bấm "Kiểm tra" (không cần đóng Chrome).' });
  } catch (e) {
    update({ activity: 'Sẵn sàng', error: e.message });
  }
}

/** Whether the profile is signed in: starts the automated Chrome when it isn't running. */
async function check() {
  if (busy) return state;
  let tab = null;
  try {
    update({ activity: 'Đang kiểm tra đăng nhập…', error: '' });
    tab = await chrome.Tab.open(await chrome.ensureAutomation());
    const url = await tab.eval('location.href').catch(() => '');
    if (!String(url).startsWith(chrome.HOME)) await navigateHome(tab);
    const ok = await tab.eval(page.LOGGED_IN);
    update({ signedIn: ok, activity: ok ? 'Sẵn sàng' : 'Chưa đăng nhập ChatGPT.' });
  } catch (e) {
    update({ activity: 'Sẵn sàng', error: e.message });
  } finally {
    if (tab) tab.close();
  }
  return state;
}

// ------------------------------------------------------------------------------------ Server

function listen() {
  token = crypto.randomBytes(32).toString('hex');
  server = http.createServer(handle);
  // An answer can take many minutes: no server-side time limit on the connection.
  server.requestTimeout = 0;
  server.timeout = 0;
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
    return reply(res, 200, { ok: true, loggedIn: state.signedIn, busy, version: VERSION });
  }
  if (req.method === 'POST' && (req.url === '/v1/image' || req.url === '/v1/collect')) {
    const collectOnly = req.url === '/v1/collect';
    log(`${req.url} received`);
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
      if (!body || (!collectOnly && (typeof body.prompt !== 'string' || !body.prompt.trim()))) return reply(res, 400, { ok: false, code: 'prompt', error: 'missing prompt' });
      // One job at a time: they share the one tab.
      const job = queue.then(() => (collectOnly ? collectJob(body) : runJob(body)));
      queue = job.catch(() => {});
      job.then(
        (r) => { log(`${req.url}: ok=${r.ok} images=${(r.images || []).length} ${r.code || ''}`); reply(res, 200, r); },
        (e) => { log(`${req.url}: ${e.code || 'error'} ${e.message || e}`); reply(res, 200, { ok: false, code: e.code || 'error', error: e.message || String(e) }); },
      );
    });
    return;
  }
  reply(res, 404, { ok: false, code: 'route', error: 'not found' });
}

// ------------------------------------------------------------------------------- The page

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(tab, code, timeoutMs, what) {
  const end = Date.now() + timeoutMs;
  for (;;) {
    try { if (await tab.eval(code)) return; } catch { /* page navigating */ }
    if (Date.now() > end) throw Object.assign(new Error(`timed out waiting for ${what}`), { code: 'timeout' });
    await sleep(400);
  }
}

/** A chatgpt.com page (a new chat, or `url`'s conversation), its composer ready. */
async function openChat(tab, url) {
  await tab.send('Page.navigate', { url });
  await sleep(1500);
  await waitFor(tab, `!!${page.COMPOSER}`, 60000, 'the ChatGPT page');
}

const navigateHome = (tab) => openChat(tab, chrome.HOME);

async function attach(tab, file) {
  const { result } = await tab.send('Runtime.evaluate', { expression: page.FILE_INPUT });
  if (!result || !result.objectId) throw Object.assign(new Error('ChatGPT page has no file upload (are you signed in?)'), { code: 'page' });
  await tab.send('DOM.setFileInputFiles', { files: [file], objectId: result.objectId });
  // The picture shows in the composer before the upload ends; Send stays disabled until it ends.
  await waitFor(tab, page.ATTACHED, 60000, 'the attached picture');
}

/** The last request's conversation, kept on disk: its answer can be taken later (`collectJob`). */
const LAST = path.join(process.env.LOCALAPPDATA || os.tmpdir(), 'ArtCraft ChatGPT', 'last-conversation.txt');

/** Notes the conversation once chatgpt.com has given it an address (soon after sending). */
async function rememberConversation(tab) {
  for (let i = 0; i < 30; i++) {
    const url = await tab.eval(page.CONVERSATION).catch(() => '');
    if (url) {
      try { fs.writeFileSync(LAST, url); } catch { /* best effort */ }
      log(`conversation ${url}`);
      return;
    }
    await sleep(1000);
  }
}

/**
 * Takes the answer of a conversation without sending anything: `body.conversation`, else the
 * last request's. For when PhotoCraft stopped waiting but ChatGPT finished.
 */
async function collectJob(body) {
  busy = true;
  let tab = null;
  const dir = path.join(os.tmpdir(), `artcraft-chatgpt-${process.pid}-${Date.now()}`);
  try {
    update({ activity: 'Đang lấy lại kết quả cho PhotoCraft…', error: '' });
    const port = await chrome.ensureAutomation();
    tab = await chrome.Tab.open(port);
    let saved = '';
    try { saved = fs.readFileSync(LAST, 'utf8').trim(); } catch { /* none yet */ }
    const here = await tab.eval(page.CONVERSATION).catch(() => '');
    // Else the conversation the tab shows (a request sent before this app noted them).
    const url = conversationUrl(body.conversation) || conversationUrl(saved) || conversationUrl(here);
    if (!url) throw Object.assign(new Error('No ChatGPT conversation to take an answer from.'), { code: 'noconversation' });
    if (here !== url) await openChat(tab, url);
    await sleep(2000);
    const s = await answer.waitAnswer(tab);
    const images = await answer.collect(port, tab, s, path.join(dir, 'files'));
    return lib.jobResult({ images, text: s.text, conversation: url });
  } catch (e) {
    update({ error: e.message });
    throw e;
  } finally {
    busy = false;
    update({ activity: 'Sẵn sàng' });
    if (tab) tab.close();
    fs.rm(dir, { recursive: true, force: true }, () => {});
  }
}

/** The conversation PhotoCraft continues, when it is one of chatgpt.com's. */
function conversationUrl(c) {
  return typeof c === 'string' && /^https:\/\/chatgpt\.com\/c\/[\w-]+$/.test(c) ? c : null;
}

async function runJob(body) {
  busy = true;
  let file = null;
  let tab = null;
  const dir = path.join(os.tmpdir(), `artcraft-chatgpt-${process.pid}-${Date.now()}`);
  try {
    update({ activity: 'Đang làm yêu cầu từ PhotoCraft…', error: '' });
    const port = await chrome.ensureAutomation();
    tab = await chrome.Tab.open(port);
    // A follow-up continues the same ChatGPT conversation, so it remembers the earlier steps.
    const conversation = conversationUrl(body.conversation);
    await openChat(tab, conversation || chrome.HOME);
    if (!(await tab.eval(page.LOGGED_IN))) {
      update({ signedIn: false });
      showWindow();
      throw Object.assign(new Error('Not signed in to ChatGPT: press "Đăng nhập bằng Chrome" in the ArtCraft ChatGPT window, then send again.'), { code: 'login' });
    }
    update({ signedIn: true });
    const hasImage = typeof body.image === 'string' && body.image.length > 0;
    if (hasImage) {
      fs.mkdirSync(dir, { recursive: true });
      file = path.join(dir, 'photocraft-canvas.png');
      fs.writeFileSync(file, Buffer.from(body.image, 'base64'));
      await attach(tab, file);
    }
    const size = Number(body.width) > 0 && Number(body.height) > 0 ? { width: Number(body.width), height: Number(body.height) } : null;
    await tab.eval(page.FOCUS);
    await tab.send('Input.insertText', { text: lib.buildPrompt(body.prompt, { hasImage, size, followUp: !!conversation }) });
    await waitFor(tab, page.SEND_READY, 180000, 'the Send button');
    const before = await tab.eval(page.USER_COUNT);
    await sleep(300);
    await tab.eval(`${page.SEND}.click()`);
    await waitFor(tab, `${page.USER_COUNT} > ${before}`, 60000, 'the request to be sent');
    rememberConversation(tab);
    const s = await answer.waitAnswer(tab);
    const images = await answer.collect(port, tab, s, path.join(dir, 'files'));
    return lib.jobResult({ images, text: s.text, conversation: await tab.eval(page.CONVERSATION) });
  } catch (e) {
    update({ error: e.message });
    throw e;
  } finally {
    busy = false;
    update({ activity: 'Sẵn sàng' });
    if (tab) tab.close();
    fs.rm(dir, { recursive: true, force: true }, () => {});
  }
}
