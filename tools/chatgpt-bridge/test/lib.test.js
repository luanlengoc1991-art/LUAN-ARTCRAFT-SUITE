'use strict';
const test = require('node:test');
const assert = require('node:assert');
const lib = require('../lib');

test('the info file lives under APPDATA', () => {
  assert.strictEqual(lib.infoPath({ APPDATA: 'C:/Users/a/AppData/Roaming' }).split(/[\\/]/).join('/'), 'C:/Users/a/AppData/Roaming/ArtCraft/chatgpt-bridge.json');
});

test('the user agent reads as Chrome, not Electron', () => {
  const ua = lib.chromeUserAgent('140.0.7339.80', 'win32');
  assert.match(ua, /Chrome\/140\.0\.0\.0/);
  assert.doesNotMatch(ua, /Electron/);
});

test('an edit asks for the edited image and keeps the request', () => {
  const p = lib.buildPrompt('Tách nền ảnh này', true);
  assert.match(p, /attached image/);
  assert.match(p, /transparent/);
  assert.match(p, /Request: Tách nền ảnh này$/);
  assert.match(lib.buildPrompt('một con mèo', false), /Create an image/);
});

test('a reply without an image tells a spent plan apart', () => {
  assert.deepStrictEqual(lib.jobResult({ image: 'AAA', mime: 'image/webp', text: '' }), { ok: true, image: 'AAA', mime: 'image/webp', text: '' });
  assert.strictEqual(lib.jobResult({ text: "You've hit the plan limit for image generation requests." }).code, 'limit');
  assert.strictEqual(lib.jobResult({ text: 'Bạn đã đạt giới hạn tạo ảnh.' }).code, 'limit');
  assert.strictEqual(lib.jobResult({ text: 'Here is a description.' }).code, 'noimage');
});

test('Chrome is found before Edge, and an override wins', () => {
  const chrome = require('../chrome');
  const env = { ProgramFiles: 'C:/PF', 'ProgramFiles(x86)': 'C:/PF86', LOCALAPPDATA: 'C:/L' };
  const norm = (p) => p && p.split(/[\\/]/).join('/');
  const has = (list) => (p) => list.includes(norm(p));
  assert.strictEqual(norm(chrome.findBrowser(env, has(['C:/PF/Google/Chrome/Application/chrome.exe', 'C:/PF86/Microsoft/Edge/Application/msedge.exe']))), 'C:/PF/Google/Chrome/Application/chrome.exe');
  assert.strictEqual(norm(chrome.findBrowser(env, has(['C:/PF86/Microsoft/Edge/Application/msedge.exe']))), 'C:/PF86/Microsoft/Edge/Application/msedge.exe');
  assert.strictEqual(chrome.findBrowser({ ...env, ARTCRAFT_CHROME: 'D:/c.exe' }, () => true), 'D:/c.exe');
  assert.strictEqual(chrome.findBrowser(env, () => false), null);
});

test('only the automated launch opens a DevTools port, on loopback, in its own profile', () => {
  const chrome = require('../chrome');
  const args = chrome.automationArgs('C:/p');
  assert.ok(args.includes('--user-data-dir=C:/p'));
  assert.ok(args.includes('--remote-debugging-address=127.0.0.1'));
  assert.ok(!args.includes('--enable-automation'), 'Google refuses sign-in to automation-flagged browsers');
});

test('the page expressions are valid JavaScript', () => {
  const page = require('../page');
  for (const [name, expr] of Object.entries(page)) {
    if (name === 'USER' || name === 'ASSISTANT') continue; // CSS selectors
    const code = typeof expr === 'function' ? expr('https://x/y.png') : expr;
    assert.doesNotThrow(() => new Function(`return (${code});`), name);
  }
});

test('only the token opens the server', () => {
  assert.ok(lib.authorized({ authorization: 'Bearer abc' }, 'abc'));
  assert.ok(!lib.authorized({ authorization: 'Bearer abd' }, 'abc'));
  assert.ok(!lib.authorized({}, 'abc'));
});
