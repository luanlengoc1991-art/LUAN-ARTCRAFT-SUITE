'use strict';
const test = require('node:test');
const assert = require('node:assert');
const lib = require('../lib');

test('the info file lives under APPDATA', () => {
  assert.strictEqual(lib.infoPath({ APPDATA: 'C:/Users/a/AppData/Roaming' }).split(/[\\/]/).join('/'), 'C:/Users/a/AppData/Roaming/ArtCraft/chatgpt-bridge.json');
});

test('the request comes first, unchanged, then how layers come back', () => {
  const p = lib.buildPrompt('Tách 9 đối tượng và 1 nền', { hasImage: true, size: { width: 941, height: 1672 } });
  assert.match(p, /^Yêu cầu của tôi: Tách 9 đối tượng và 1 nền\n/);
  assert.match(p, /941 × 1672 px/);
  assert.match(p, /MỖI FILE THÀNH MỘT LAYER RIÊNG/);
  assert.match(p, /KHÔNG gửi ZIP/);
  assert.match(p, /```json/);
  assert.match(lib.buildPrompt('một con mèo'), /tạo ảnh mới/);
  const next = lib.buildPrompt('làm sáng cô dâu', { followUp: true });
  assert.match(next, /^làm sáng cô dâu\n/);
  assert.ok(next.length < 300);
});

test('only image files come back, in their numbered order', () => {
  assert.deepStrictEqual(lib.imageFiles(['10_hoa.png', 'layers.zip', '02_co_dau.PNG', '01_nen.png', 'note.txt', '03_x.webp', 'senturia_recombined.png']), ['01_nen.png', '02_co_dau.PNG', '03_x.webp', '10_hoa.png']);
});

test('a reply without an image tells a spent plan apart', () => {
  const ok = lib.jobResult({ images: [{ name: 'a.png', image: 'AAA', mime: 'image/png' }, { name: 'b.png', image: 'BBB', mime: 'image/png' }], text: 'x', conversation: 'https://chatgpt.com/c/1' });
  assert.strictEqual(ok.ok, true);
  assert.strictEqual(ok.images.length, 2);
  assert.strictEqual(ok.image, 'AAA');
  assert.strictEqual(ok.conversation, 'https://chatgpt.com/c/1');
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
