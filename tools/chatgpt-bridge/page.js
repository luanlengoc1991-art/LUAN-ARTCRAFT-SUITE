// Expressions run inside chatgpt.com. When the site changes its page, these are what to update.
'use strict';

// ChatGPT's composer has had several forms (ProseMirror #prompt-textarea, a plain textarea).
const COMPOSER = `document.querySelector('#prompt-textarea, #mobile-composer-prompt, div[contenteditable="true"].ProseMirror, form textarea')`;

/** The file input for pictures. */
const FILE_INPUT = `(() => {
  const all = [...document.querySelectorAll('input[type=file]')];
  return document.querySelector('#upload-photos')
    || all.find(i => (i.accept || '').includes('image') && !i.hasAttribute('capture'))
    || all.find(i => !i.hasAttribute('capture')) || all[0] || null;
})()`;

/** The attached picture shows in the composer. */
const ATTACHED = `(() => { const c = ${COMPOSER}; const f = c && (c.closest('form') || c.parentElement.parentElement.parentElement); return !!(f && f.querySelector('img')); })()`;

const FOCUS = `(() => { const c = ${COMPOSER}; if (!c) return false; c.focus(); return true; })()`;

const SEND = `(() => {
  const c = ${COMPOSER};
  const f = c && c.closest('form');
  return document.querySelector('[data-testid="send-button"], #composer-submit-button') || (f && f.querySelector('button[type=submit]')) || document.querySelector('button[aria-label^="Send"]');
})()`;

const SEND_READY = `(() => { const b = ${SEND}; return !!(b && !b.disabled && b.getAttribute('aria-disabled') !== 'true'); })()`;

// Messages are marked data-message-author-role (older page), li[data-message-role] (signed
// out) or data-content-search-unit-key="…:user" / "…:assistant" (signed in, 2026).
const USER = `[data-message-author-role="user"], [data-message-role="user"], [data-content-search-unit-key$=":user"]`;
const ASSISTANT = `[data-message-author-role="assistant"], [data-message-role="assistant"], [data-content-search-unit-key$=":assistant"]`;

/** How many requests the conversation shows. */
const USER_COUNT = `document.querySelectorAll('${USER}').length`;

const LOGGED_IN = `fetch('/api/auth/session', {credentials: 'include'}).then(r => r.ok ? r.json() : {}).then(s => !!(s && s.user)).catch(() => false)`;

/** The page's state after the last request: the images ChatGPT gave (largest last), whether it's still working, its words. */
const STATE = `(() => {
  const users = document.querySelectorAll('${USER}');
  const last = users[users.length - 1];
  if (!last) return null;
  const after = (n) => !!(last.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING) && !last.contains(n);
  const blurred = (n) => { for (let e = n, i = 0; e && i < 5; e = e.parentElement, i++) { const f = getComputedStyle(e).filter || ''; if (f.includes('blur')) return true; } return false; };
  // Not the user's own picture (its turn holds the user message) nor the composer's preview.
  const theirs = (i) => { const t = i.closest('article, li, [data-testid^="conversation-turn"], [data-content-search-unit-key]'); return !i.closest('form') && !(t && (t.matches('${USER}') || t.querySelector('${USER}'))); };
  const attachment = (i) => /user attachment|uploaded image/i.test(i.alt || '');
  const generated = (i) => /generated image|ảnh được tạo|hình ảnh được tạo/i.test(i.alt || '');
  const imgs = [...document.querySelectorAll('img')].filter(i => after(i) && theirs(i) && !attachment(i) && !i.closest('nav, aside, header') && i.complete && i.naturalWidth >= 256 && i.naturalHeight >= 256);
  // Several images come as a gallery: "Show generated image N" buttons whose thumbnails load only
  // when scrolled to, so they count by their number, loaded or not.
  const numbered = (b) => parseInt(((b.getAttribute('aria-label') || '').match(/(\\d+)\\s*$/) || [])[1] || '0', 10);
  const gallery = [...document.querySelectorAll('button[aria-label^="Show generated image"] img, button[aria-label^="Hiển thị hình ảnh"] img, button[aria-label^="Xem hình ảnh"] img')]
    .filter(i => after(i) && (i.currentSrc || i.src))
    .sort((a, b) => numbered(a.closest('button')) - numbered(b.closest('button')));
  // Every image of the answer, in order, once each (a gallery shows one twice).
  const srcs = [...new Set((gallery.length ? gallery : imgs.filter(i => generated(i) || i.naturalWidth >= 512)).map(i => i.currentSrc || i.src))];
  // Files the answer made (Python): a "Download file" button each.
  const files = [...document.querySelectorAll('button')].filter(b => after(b) && !b.closest('nav, aside, header, form') && /^(download file|tải xuống tệp|tải tệp xuống|tải xuống)$/i.test((b.getAttribute('aria-label') || '').trim())).length;
  // ChatGPT's own images ("Generated image 1") last, then by size: the last one is the answer.
  imgs.sort((a, b) => (generated(a) - generated(b)) || (a.naturalWidth * a.naturalHeight - b.naturalWidth * b.naturalHeight));
  const answers = [...document.querySelectorAll('${ASSISTANT}')].filter(after);
  // Each answer starts with a screen-reader heading ("ChatGPT said:"), in the page's language.
  const text = answers.map(a => a.innerText.trim().replace(/^[^\\n]{0,40}:\\n/, '').trim()).filter(Boolean).join('\\n').slice(0, 2000);
  const lastAnswer = answers[answers.length - 1];
  const turn = lastAnswer ? (lastAnswer.closest('article, li') || lastAnswer) : null;
  const states = [...document.querySelectorAll('[data-talvt-turn-state]')].filter(after);
  const lastState = states.length ? states[states.length - 1].getAttribute('data-talvt-turn-state') : 'complete';
  const working = !!document.querySelector('[data-testid="stop-button"], button[aria-label="Stop streaming"], button[aria-label="Stop generating"], button[aria-label^="Stop"]')
    || lastState !== 'complete'
    || !!(lastAnswer && lastAnswer.matches('[data-message-role]') && !lastAnswer.hasAttribute('data-message-complete'))
    || !!(turn && turn.querySelector('[class*="shimmer"], [class*="animate-pulse"], [aria-busy="true"]'))
    || imgs.some(blurred);
  const best = imgs[imgs.length - 1];
  // An answer that is only an image has no assistant text block.
  return { src: best ? best.currentSrc || best.src : '', srcs, files, count: imgs.length, working, answered: answers.length > 0 || imgs.some(generated), text };
})()`;

/** The buttons of the last answer: "8 more" (shows the rest of its files) and "Download file". */
const ANSWER_BUTTONS = `(() => {
  const users = document.querySelectorAll('${USER}');
  const last = users[users.length - 1];
  return [...document.querySelectorAll('button')].filter(b => (!last || (last.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING)) && !b.closest('nav, aside, header, form'));
})()`;

/** Shows every file of the answer ("8 more", "8 tệp khác"…); how many such buttons were pressed. */
const EXPAND_FILES = `(() => { const bs = ${ANSWER_BUTTONS}.filter(b => /^\\s*(\\+\\s*)?\\d+\\s+(more|tệp khác|khác|nữa)\\s*$|^\\s*(xem thêm|thêm \\d+)/i.test(b.innerText || '')); bs.forEach(b => b.click()); return bs.length; })()`;

const DOWNLOAD_BUTTONS = `${ANSWER_BUTTONS}.filter(b => /^(download file|tải xuống tệp|tải tệp xuống|tải xuống)$/i.test((b.getAttribute('aria-label') || '').trim()))`;

/** How many "Download file" buttons the last answer has. */
const DOWNLOAD_COUNT = `${DOWNLOAD_BUTTONS}.length`;

/** Presses the n-th "Download file" button of the answer; false when there is none. */
const clickDownload = (n) => `(() => { const b = ${DOWNLOAD_BUTTONS}[${Number(n)}]; if (!b) return false; b.scrollIntoView({block: 'center'}); b.click(); return true; })()`;

/** The conversation's address, to continue it next time. */
const CONVERSATION = `location.href.startsWith('https://chatgpt.com/c/') ? location.href.split('?')[0] : ''`;

/** An image read inside the page (same-origin, signed-in), as a data: URL. */
const readImage = (src) => `fetch(${JSON.stringify(src)}, {credentials: 'include'}).then(r => { if (!r.ok) throw new Error('HTTP ' + r.status); return r.blob(); }).then(b => new Promise((ok, no) => { const f = new FileReader(); f.onload = () => ok(f.result); f.onerror = no; f.readAsDataURL(b); }))`;

module.exports = { COMPOSER, FILE_INPUT, ATTACHED, FOCUS, SEND, SEND_READY, USER, ASSISTANT, USER_COUNT, LOGGED_IN, STATE, EXPAND_FILES, DOWNLOAD_COUNT, clickDownload, CONVERSATION, readImage };
