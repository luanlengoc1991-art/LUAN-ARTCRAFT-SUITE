// Pure helpers of ArtCraft ChatGPT (no Electron): tested with `npm test`.
'use strict';

const path = require('path');

/** Where a running bridge says how to reach it: %APPDATA%\ArtCraft\chatgpt-bridge.json. */
function infoPath(env = process.env) {
  const base = env.APPDATA || path.join(env.HOME || env.USERPROFILE || '.', '.config');
  return path.join(base, 'ArtCraft', 'chatgpt-bridge.json');
}

/**
 * What ChatGPT is asked: the user's own request, first and unchanged, then how PhotoCraft takes
 * the answer back (every image file becomes its own layer, in order, at its place on the canvas).
 * `size` is the attached canvas in pixels; `followUp` when the conversation already has the rules.
 */
function buildPrompt(request, { hasImage = false, size = null, followUp = false } = {}) {
  const req = String(request || '').trim();
  const canvas = size && size.width && size.height ? `${size.width} × ${size.height} px` : null;
  if (followUp) {
    return [req, '', '(PhotoCraft: trả kết quả như trước — mỗi lớp một file PNG, lớp đối tượng nền trong suốt, lớp nền đầy đủ; cùng kích thước canvas, đúng vị trí, thứ tự từ dưới lên; kèm khối JSON vị trí.)'].join('\n');
  }
  const rules = [
    'Bối cảnh: tôi gửi yêu cầu này từ PhotoCraft (phần mềm chỉnh ảnh kiểu Photoshop). Mọi file ảnh bạn trả về sẽ được tự động đưa vào PhotoCraft, MỖI FILE THÀNH MỘT LAYER RIÊNG (Smart Object), xếp theo đúng thứ tự file: file đầu tiên nằm dưới cùng.',
    hasImage ? `Ảnh đính kèm là canvas hiện tại${canvas ? ` (${canvas})` : ''}.` : 'Hiện chưa có ảnh nào: hãy tạo ảnh mới.',
    '',
    'Cách trả kết quả:',
    '- Trả kết quả dưới dạng file ảnh PNG (ảnh hiển thị trong câu trả lời hoặc file tải về), KHÔNG gửi ZIP.',
    '- Giữ độ phân giải cao nhất có thể, không nén giảm chất lượng, ảnh sắc nét.',
    '- Khi tách layer / tách đối tượng: mỗi đối tượng là MỘT file PNG RIÊNG, nền trong suốt, tách theo đúng đường viền thật của đối tượng (tóc, váy, hoa, chữ…), KHÔNG cắt theo ô chữ nhật hay theo vùng ảnh.',
    `- Mỗi file layer có CÙNG kích thước với canvas${canvas ? ` (${canvas})` : ''} và đối tượng nằm ĐÚNG vị trí cũ, để các layer chồng lên nhau ghép lại đúng ảnh gốc.`,
    '- Nếu cần lớp nền: file nền là file đầu tiên và là ảnh ĐẦY ĐỦ, KHÔNG trong suốt (chỉ các lớp đối tượng mới trong suốt); phần đối tượng đã lấy ra được tô lấp (inpaint) tự nhiên, không để lỗ.',
    '- Kiểm tra lại trước khi trả: không file nào được trống hoàn toàn.',
    '- Đặt tên file có số thứ tự và tên đối tượng, ví dụ 01_nen.png, 02_co_dau.png, 03_bo_hoa.png.',
    '- Cuối câu trả lời, ghi một khối ```json gồm danh sách [{"file": "02_co_dau.png", "name": "Cô dâu", "x": 0, "y": 0, "w": 0, "h": 0}] theo đúng thứ tự file; x, y, w, h là khung chứa đối tượng tính bằng pixel trên canvas gốc.',
    '- Khi chỉ chỉnh sửa hoặc tạo một ảnh: trả một file PNG.',
  ];
  return [`Yêu cầu của tôi: ${req}`, '', ...rules].join('\n');
}

/** Whether ChatGPT's answer says the plan has no usage left for now. */
function isLimit(text) {
  const t = String(text || '').toLowerCase();
  return ['usage limit', 'limit reached', 'reached the limit', 'hit the', 'quota', 'try again later', 'hết hạn mức', 'đạt giới hạn', 'thử lại sau'].some((k) => t.includes(k));
}

/** The reply for a finished job: the images (in order), or why there are none. */
function jobResult({ images = [], text = '', conversation = '' }) {
  if (images.length) {
    const first = images[0];
    return { ok: true, images, image: first.image, mime: first.mime, text, conversation };
  }
  if (isLimit(text)) return { ok: false, code: 'limit', error: `ChatGPT usage limit reached: ${text}`.trim(), text, conversation };
  return { ok: false, code: 'noimage', error: text ? `ChatGPT answered without an image: ${text}` : 'ChatGPT answered without an image.', text, conversation };
}

/**
 * The layer files, in their names' order (01_…, 02_…): images only (no ZIPs or documents), and
 * not the recombined preview ChatGPT adds to show the layers put back together.
 */
function imageFiles(names) {
  const preview = /(recombined|recomposed|composite|combined|merged|flatten|preview|ghep|ghép|tong_hop|tổng_hợp)/i;
  return names.filter((n) => /\.(png|jpe?g|webp)$/i.test(n) && !preview.test(n)).sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
}

/** Whether a request carries this bridge's token. */
function authorized(headers, token) {
  const h = headers && (headers.authorization || headers.Authorization);
  return typeof h === 'string' && h === `Bearer ${token}`;
}

module.exports = { infoPath, buildPrompt, isLimit, jobResult, imageFiles, authorized };
