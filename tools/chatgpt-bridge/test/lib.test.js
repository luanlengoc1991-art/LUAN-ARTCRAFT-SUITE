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

test('only the token opens the server', () => {
  assert.ok(lib.authorized({ authorization: 'Bearer abc' }, 'abc'));
  assert.ok(!lib.authorized({ authorization: 'Bearer abd' }, 'abc'));
  assert.ok(!lib.authorized({}, 'abc'));
});
