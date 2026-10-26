// Pure helpers of ArtCraft ChatGPT (no Electron): tested with `node --test test`.
'use strict';

const path = require('path');

/** Where a running bridge says how to reach it: %APPDATA%\ArtCraft\chatgpt-bridge.json. */
function infoPath(env = process.env) {
  const base = env.APPDATA || path.join(env.HOME || env.USERPROFILE || '.', '.config');
  return path.join(base, 'ArtCraft', 'chatgpt-bridge.json');
}

/** A Chrome user agent without "Electron": sign-in pages refuse browsers they don't know. */
function chromeUserAgent(chromeVersion, platform = process.platform) {
  const major = String(chromeVersion || '140').split('.')[0];
  const os = platform === 'darwin' ? 'Macintosh; Intel Mac OS X 10_15_7' : platform === 'linux' ? 'X11; Linux x86_64' : 'Windows NT 10.0; Win64; x64';
  return `Mozilla/5.0 (${os}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${major}.0.0.0 Safari/537.36`;
}

/** What ChatGPT is asked: the user's words, with the image handed back as an image. */
function buildPrompt(request, hasImage) {
  const req = String(request || '').trim();
  if (!hasImage) {
    return `Create an image for this request and reply with the image (use your image generation tool):\n\n${req}`;
  }
  return [
    'Edit the attached image as requested below and reply with the edited image (use your image generation tool).',
    'Keep the same subject, framing and aspect ratio unless the request says otherwise.',
    'If the request removes or cuts out the background (e.g. "tách nền", "xóa nền"), make the background fully transparent (PNG with alpha).',
    '',
    `Request: ${req}`,
  ].join('\n');
}

/** Whether ChatGPT's answer says the plan has no usage left for now. */
function isLimit(text) {
  const t = String(text || '').toLowerCase();
  return ['limit', 'quota', 'try again later', 'giới hạn', 'hạn mức', 'thử lại sau'].some((k) => t.includes(k));
}

/** The reply for a finished job: the image, or why there is none. */
function jobResult({ image, mime, text }) {
  if (image) return { ok: true, image, mime: mime || 'image/png', text: text || '' };
  if (isLimit(text)) return { ok: false, code: 'limit', error: `ChatGPT usage limit reached: ${text}`.trim(), text: text || '' };
  return { ok: false, code: 'noimage', error: text ? `ChatGPT answered without an image: ${text}` : 'ChatGPT answered without an image.', text: text || '' };
}

/** Whether a request carries this bridge's token. */
function authorized(headers, token) {
  const h = headers && (headers.authorization || headers.Authorization);
  return typeof h === 'string' && h === `Bearer ${token}`;
}

module.exports = { infoPath, chromeUserAgent, buildPrompt, isLimit, jobResult, authorized };
