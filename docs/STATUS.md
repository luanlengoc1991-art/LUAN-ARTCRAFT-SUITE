# Báo cáo trạng thái: 2026-10-14

Mọi kết quả dưới đây đã **chạy và kiểm tra thật** trên máy Windows 11 x64 này, trừ chỗ ghi rõ khác.

## Đang làm (15/10, chiều): nâng cấp AI & MCP cho PhotoCraft + VectorCraft

Yêu cầu gốc: thêm "AI / MCP Command Center" (Trò chuyện AI, Bảng điều khiển MCP, Kết nối) vào
PhotoCraft và VectorCraft, rồi bổ sung tính năng còn thiếu. Quy tắc: không đăng nhập mới, không API
key, không đọc token, không công cụ giả, không viết lại MCP. Chủ repo chỉ cần tiếng Việt + tiếng Anh,
và muốn mọi trả lời bằng tiếng Việt.

- **Nhóm 1 — XONG: VectorCraft** (`apps/vectorcraft/crates/ui-egui/src/panels/ai_center.rs`).
  Menu Cửa sổ → Trung tâm lệnh AI / MCP. Gửi lệnh qua CLI chính thức của agent (`claude -p`
  với `--mcp-config` trỏ `vectorcraft-cli mcp --connect` vào cửa sổ, `--resume` để nhớ ngữ cảnh),
  Giữ/Hoàn tác thay đổi, lưu lịch sử `%APPDATA%\VectorCraft\ai-chats.json`. Console chạy
  `vectorcraft_mcp::call_tool` ngay trong app. Kênh điều khiển bật lúc chạy: `VectorcraftApp::ensure_control`.
  Đã chạy thật với Claude Code (vẽ, đổi màu, trace logo). Codex CLI / Cursor Agent chưa thử (máy chưa cài).
- **Nhóm 2 — XONG: PhotoCraft** (`apps/photocraft/crates/ui-egui/src/ai_center.rs`, cửa sổ nổi;
  menu Window → AI / MCP Command Center). Console là MCP client tới
  `photocraft-cli mcp --bridge 127.0.0.1:PORT --control-token-file F`, chạy ở luồng nền (UI phải
  rảnh để trả lời kênh điều khiển). Kênh điều khiển bật lúc chạy: `PhotocraftApp::ensure_control`,
  cổng 7971 + token `%LOCALAPPDATA%\ArtCraft\artcraft-mcp\photocraft.token` (trùng ArtCraft MCP).
  Đã chạy thật: Claude tạo layer và vẽ hình chữ nhật xanh ngọc theo lệnh tiếng Việt.
- **Ngôn ngữ (16/10):** PhotoCraft và VectorCraft chỉ cho chọn English (mặc định) và Tiếng Việt:
  `i18n::offer_only_english_and_vietnamese()` gọi trong `main.rs`; test không gọi nên giữ hành vi gốc.
- **Nhóm 3 — CHƯA LÀM: tính năng PhotoCraft còn thiếu:** Upscale, tách nhiều đối tượng thành PNG riêng,
  xuất hàng loạt PNG/ZIP. Đã có sẵn: layer, mask, Smart Object, 134 filter, `layer.removeBackground`,
  `select.subject/object`, healing, `file.automate.batch`, `file.export.layersToFiles`.
- **Nhóm 4 — CHƯA LÀM: VectorCraft còn thiếu:** Auto Layout, xuất hàng loạt SVG/PDF/PNG, kiểm tra 3D Extrude.
  Đã có: Pathfinder, Shape Builder, Gradient, Type, Perspective, Image Trace, Recolor, Align/Distribute.
- Generative Fill / xóa vật thể bằng AI cần mô hình ảnh: hiện "Không hỗ trợ", không làm giả.
- Kiểm thử VectorCraft: `cargo test -p vectorcraft-ui-egui --lib` (1268 test), clippy, `cargo xtask layers`,
  wasm. Chuỗi giao diện mới phải có trong mọi file `i18n/*.tsv` (test bắt buộc); chuỗi kỹ thuật thì thêm
  vào `NOT_MESSAGES` trong `i18n/tests.rs`.

## Cập nhật 15/10: MCP tổng (ArtCraft MCP)

- `tools/artcraft-mcp`: một MCP server cho mọi app đã cài. Tool có tên dạng `<app>_<tool>`. Có
  thêm `artcraft_open` để mở cửa sổ app và điều khiển trực tiếp. Không sửa app nào.
- Link cố định: https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/latest/download/ArtCraft-MCP.exe,
  do workflow `artcraft-mcp.yml` phát hành. Cài và đăng ký vào Claude Code + Cursor bằng `scripts\install-mcp.ps1`.
- Đã kiểm tra thật với PhotoCraft, VectorCraft, WordCraft và GridCraft. Chi tiết trong `docs/MCP-AUTOMATION.md`.

## Cập nhật (tối 14/10): độc lập + trình quản lý

- **Thư viện git đã vendor** vào `vendor/git/` (10 bộ, khoảng 20 MB). Với CARGO_HOME trống, DesignCraft,
  PhotoCraft và EffectCraft tải 0 repo git. Xem `docs/INDEPENDENCE.md`.
- **Trình quản lý** `apps/craft-launcher` (ArtCraft Launcher 0.4.1): cài và cập nhật app từ
  `luanlengoc1991-art/<app>`, đã tắt feed `api.storyteller.ai`. `cargo test --workspace` đạt toàn bộ.
- **Releases đã phát hành** (`.msi` + `.zip` portable + `SHA256SUMS.txt`):
  photocraft v0.9.0, vectorcraft v0.11.0, filmcraft v0.8.0, pdfcraft v0.8.0, effectcraft v0.10.0,
  wordcraft v0.7.0, cadcraft v0.7.0, gridcraft v0.7.0, craft-launcher v0.4.1.
- Kiểm chứng bằng mã của launcher trên dữ liệu GitHub thật: cả 9 đều chọn được bản `.msi` Windows x64
  kèm checksum. **Chưa** thử cài thật bằng giao diện launcher.
- VectorCraft đã nhận 3 tính năng từ fork cũ của bạn (tự cập nhật, autobuild, CLI cho MCP). Các fork
  cũ được đổi tên thành `photocraft-fork-old` và `vectorcraft-fork-old`.
- Bản cài **chưa ký số**, nên Windows SmartScreen sẽ cảnh báo lần đầu.

## Tích hợp

- **13/13 app** đã nhập vào `apps/<id>/` bằng git subtree (squash) từ `github.com/storytold/<id>`, nhánh `main`.
- Sau khi nhập, đã kiểm tra cây thư mục của từng app trùng khớp với commit upstream.
- GitHub: https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE (public, `origin`).

## Build Windows x64 (Rust 1.99.0, MSVC 2022 Build Tools)

Bộ build gồm 8 app theo lựa chọn của bạn. Đầu ra nằm ở `builds/<id>/<version>/windows-x64/`.

| App | Phiên bản | Build | GUI .exe | CLI .exe | MCP chạy thật (tools) |
|-----|-----------|-------|----------|----------|------------------------|
| PhotoCraft | 0.9.0 | ✔ | 75 MB | 48 MB | ✔ 25 |
| VectorCraft | 0.11.0 | ✔ | 95 MB | 61 MB | ✔ 32 |
| FilmCraft | 0.8.0 | ✔ | 58 MB | 43 MB | ✔ 17 |
| PdfCraft | 0.8.0 | ✔ | 65 MB | 40 MB | ✔ 163 |
| EffectCraft | 0.10.0 | ✔ | 85 MB | 84 MB | ✔ 27 |
| WordCraft | 0.7.0 | ✔ | 59 MB | 33 MB | ✔ 16 |
| CADCraft | 0.7.0 | ✔ | 48 MB | 27 MB | ✔ 12 |
| GridCraft | 0.7.0 | ✔ | 32 MB | 14 MB | ✔ 21 |

"MCP chạy thật" nghĩa là `<id>-cli.exe mcp` đã trả lời `initialize` (protocol 2025-06-18) và
`tools/list`. Chưa gọi thử từng tool.

**Không build (theo yêu cầu):** LightCraft, DesignCraft, FigCraft, SoundCraft, DeckCraft. Mã nguồn
vẫn giữ và vẫn được đồng bộ upstream. Theo source, 5 app này cũng có CLI + MCP, nhưng chưa chạy thử.

**Chưa làm:** file `.msi` (cần .NET SDK + WiX v5, chưa cài). Chưa chạy `cargo test` đầy đủ, mới
kiểm tra biên dịch cho các app được nâng cấp.

## Đồng bộ upstream

Lần nâng cấp đầu bằng `scripts\upgrade.ps1`: PhotoCraft 0.8.2→0.9.0, FilmCraft 0.7.2→0.8.0,
EffectCraft 0.9.2→0.10.0, LightCraft (commit mới, cùng 0.8.0). Không có xung đột. Đã merge và push.
Upstream commit nhiều lần mỗi ngày, nên chạy `scripts\upstream-status.ps1` để xem trạng thái hiện tại.

## Sự cố đã xử lý

- Một dòng thử nghiệm (`version = "0.8.2-luan"`) từ bài test xung đột đã lọt vào commit
  `f9ccff6ed` và lên GitHub. Đã khôi phục bằng commit `d51ed9a4a` và kiểm tra lại cả 13 app trùng
  khớp upstream. `upstream-status` vẫn đếm 2 "local commits" cho PhotoCraft vì hai commit này
  triệt tiêu nhau. Nội dung thực tế không có chỉnh sửa riêng.
- Đã thêm quy tắc chỉ `git add` đường dẫn cụ thể vào `CLAUDE.md`.

## Còn mở

- MSI installer: cài .NET 8 SDK + WiX 5.0.2 rồi chạy `scripts\build-app.ps1 -App <id> -Msi`.
- GitHub Actions: CI đã chạy trên GitHub, và job `detect` chọn đúng các app thay đổi. Lần test
  4 app vừa nâng cấp (run 38588842831) đang chạy lúc viết báo cáo, nên chưa có kết quả. Xem tab
  Actions. `upstream-check` (hằng ngày) và `build-windows` (chạy tay) chưa chạy lần nào.
- MCP Gateway: mới là đề xuất (`docs/MCP-AUTOMATION.md`), chưa triển khai.
- Trình cài Build Tools khuyên khởi động lại máy.
