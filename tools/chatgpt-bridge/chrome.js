// The user's own Chrome (or Edge) with a profile of its own, for chatgpt.com.
//
// Google refuses to sign in inside an embedded browser ("This browser or app may not be
// secure"), so the sign-in happens in real Chrome: once, in this profile, opened WITHOUT
// automation. Requests then start the same profile with a DevTools port on loopback and drive
// its chatgpt.com tab; the sign-in cookies stay in the profile, so it signs in only once.
'use strict';

const { execFile, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const HOME = 'https://chatgpt.com/';

function profileDir(env = process.env) {
  return path.join(env.LOCALAPPDATA || env.HOME || '.', 'ArtCraft ChatGPT', 'chrome-profile');
}

/** chrome.exe (or msedge.exe): `ARTCRAFT_CHROME` first, then the usual install folders. */
function findBrowser(env = process.env, exists = fs.existsSync) {
  const pf = env.ProgramFiles || 'C:\\Program Files';
  const pf86 = env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)';
  const local = env.LOCALAPPDATA || '';
  const candidates = [
    env.ARTCRAFT_CHROME,
    path.join(pf, 'Google', 'Chrome', 'Application', 'chrome.exe'),
    path.join(pf86, 'Google', 'Chrome', 'Application', 'chrome.exe'),
    local && path.join(local, 'Google', 'Chrome', 'Application', 'chrome.exe'),
    path.join(pf86, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
    path.join(pf, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
  ];
  return candidates.find((p) => p && exists(p)) || null;
}

/** Flags shared by both launches: this profile, no first-run pages. */
function baseArgs(profile) {
  return [`--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check'];
}

/** The automated launch: a loopback DevTools port, and no slowing down when in the background. */
function automationArgs(profile) {
  return [
    ...baseArgs(profile),
    '--remote-debugging-port=0',
    '--remote-debugging-address=127.0.0.1',
    // The page sees an ordinary browser (navigator.webdriver stays false).
    '--disable-blink-features=AutomationControlled',
    // Extensions Chrome Sync brings into the profile (e.g. new-tab overlays) stay out of the way.
    '--disable-extensions',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--window-size=1100,850',
    // Works out of sight; the page still counts as visible, so nothing is paused.
    '--window-position=-32000,-32000',
    HOME,
  ];
}

/** The DevTools port of a running automated launch (Chrome writes it into the profile). */
function readPort(profile) {
  try {
    const port = parseInt(fs.readFileSync(path.join(profile, 'DevToolsActivePort'), 'utf8').split(/\r?\n/)[0], 10);
    return Number.isInteger(port) && port > 0 ? port : null;
  } catch {
    return null;
  }
}

async function alive(port) {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(2000) });
    return r.ok;
  } catch {
    return false;
  }
}

/** Closes every Chrome process that uses this profile (its sign-in window or an automated one). */
function closeProfile(profile) {
  const needle = profile.replace(/'/g, "''");
  const ps = `Get-CimInstance Win32_Process -Filter "Name='chrome.exe' or Name='msedge.exe'" | Where-Object { $_.CommandLine -like '*${needle}*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`;
  return new Promise((resolve) => execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', ps], { windowsHide: true }, () => resolve()));
}

/** Opens chatgpt.com's sign-in in this profile, as a normal Chrome window (no automation). */
async function openSignIn() {
  const exe = findBrowser();
  if (!exe) throw Object.assign(new Error('Chrome or Edge is needed to sign in to ChatGPT.'), { code: 'browser' });
  const profile = profileDir();
  await closeProfile(profile);
  try { fs.unlinkSync(path.join(profile, 'DevToolsActivePort')); } catch { /* none */ }
  spawn(exe, [...baseArgs(profile), `${HOME}auth/login`], { detached: true, stdio: 'ignore' }).unref();
}

/** The DevTools port of the automated profile, starting it when needed. */
async function ensureAutomation() {
  const exe = findBrowser();
  if (!exe) throw Object.assign(new Error('Chrome or Edge is needed for ArtCraft ChatGPT.'), { code: 'browser' });
  const profile = profileDir();
  const running = readPort(profile);
  if (running && (await alive(running))) return running;
  // The sign-in window (or a stale launch) holds the profile: close it, the cookies are saved.
  await closeProfile(profile);
  try { fs.unlinkSync(path.join(profile, 'DevToolsActivePort')); } catch { /* none */ }
  fs.mkdirSync(profile, { recursive: true });
  spawn(exe, automationArgs(profile), { detached: true, stdio: 'ignore' }).unref();
  const end = Date.now() + 30000;
  while (Date.now() < end) {
    await new Promise((r) => setTimeout(r, 300));
    const port = readPort(profile);
    if (port && (await alive(port))) return port;
  }
  throw Object.assign(new Error('Chrome did not start with its DevTools port.'), { code: 'browser' });
}

/** A DevTools connection to one tab. */
class Tab {
  static async open(port) {
    let list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    let page = list.find((t) => t.type === 'page' && t.url.startsWith(HOME)) || list.find((t) => t.type === 'page');
    if (!page) page = await (await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(HOME)}`, { method: 'PUT' })).json();
    const tab = new Tab(page.webSocketDebuggerUrl);
    await tab.ready;
    return tab;
  }

  /** The browser itself (downloads), rather than a tab. */
  static async browser(port) {
    const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    const b = new Tab(v.webSocketDebuggerUrl);
    await b.ready;
    return b;
  }

  constructor(url) {
    this.id = 0;
    this.pending = new Map();
    this.listeners = [];
    this.ws = new WebSocket(url);
    this.ready = new Promise((resolve, reject) => {
      this.ws.onopen = resolve;
      this.ws.onerror = () => reject(new Error('could not reach Chrome'));
    });
    this.ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.method) {
        for (const fn of this.listeners) fn(d.method, d.params || {});
        return;
      }
      const p = d.id && this.pending.get(d.id);
      if (!p) return;
      this.pending.delete(d.id);
      if (d.error) p.reject(new Error(d.error.message)); else p.resolve(d.result);
    };
    this.ws.onclose = () => {
      for (const p of this.pending.values()) p.reject(new Error('Chrome closed'));
      this.pending.clear();
    };
  }

  send(method, params = {}) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  /** The value of a page expression (promises awaited). */
  async eval(expression) {
    const r = await this.send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  }

  close() {
    try { this.ws.close(); } catch { /* closed */ }
  }
}

module.exports = { HOME, profileDir, findBrowser, automationArgs, readPort, openSignIn, ensureAutomation, closeProfile, Tab };
