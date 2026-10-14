# Báo cáo kiểm tra mã nguồn: 13 ứng dụng ArtCraft

Ngày kiểm tra: 2026-10-14. Nguồn: https://getartcraft.com/apps và tổ chức GitHub
https://github.com/storytold (được chính website liên kết tới ở chân trang và nút "Browse on GitHub").

## Cách xác minh repository

- Website liệt kê đúng 13 app. Không có link GitHub riêng cho từng app, nhưng trỏ tới tổ chức
  `storytold` và ArtCraft Launcher (`storytold/craft-launcher`).
- README của Launcher liệt kê đúng 13 app này là "Craft App Suite".
- Trong `storytold` có đúng 13 repo trùng tên app, mỗi repo có mô tả khớp với app tương ứng,
  giấy phép Apache-2.0 (thực tế là MIT OR Apache-2.0) và commit mới trong ngày kiểm tra.
- Không thiếu app nào. Không thay thế app nào.

## Bảng tổng hợp

Commit = commit upstream đã nhập vào monorepo (nhánh `main`).

| # | App | Repository | Phiên bản | Commit nhập | Rust tối thiểu | GUI | CLI | MCP | Win x64 MSI |
|---|-----|------------|-----------|-------------|----------------|-----|-----|-----|-------------|
| 1 | PhotoCraft | storytold/photocraft | 0.8.2 | 7250e6e29a | 1.95 | ✔ | ✔ | ✔ | ✔ (WiX v5) |
| 2 | VectorCraft | storytold/vectorcraft | 0.11.0 | 0a33cff3f6 | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 3 | FilmCraft | storytold/filmcraft | 0.7.2 | cee1feb185 | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 4 | LightCraft | storytold/lightcraft | 0.8.0 | 16dbb90499 | 1.90 | ✔ | ✔ | ✔ | ✔ |
| 5 | PdfCraft | storytold/pdfcraft | 0.8.0 | 9931329b9f | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 6 | EffectCraft | storytold/effectcraft | 0.9.2 | 549e9d8251 | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 7 | DesignCraft | storytold/designcraft | 0.8.0 | 11a51d5b66 | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 8 | FigCraft | storytold/figcraft | 0.6.0 | 833f0d2c27 | 1.95 | ✔ | ✔ | ✔ | ✔ |
| 9 | WordCraft | storytold/wordcraft | 0.7.0 | fc41918814 | 1.90 | ✔ | ✔ | ✔ | ✔ |
| 10 | SoundCraft | storytold/soundcraft | 0.7.0 | 3a28a65769 | 1.90 | ✔ | ✔ | ✔ | ✔ |
| 11 | DeckCraft | storytold/deckcraft | 0.7.0 | d78e657e4c | 1.90 | ✔ | ✔ | ✔ | ✔ |
| 12 | CADCraft | storytold/cadcraft | 0.7.0 | fc4e98315a | 1.90 | ✔ | ✔ | ✔ | ✔ |
| 13 | GridCraft | storytold/gridcraft | 0.7.0 | d9d09f0baa | 1.90 | ✔ | ✔ | ✔ | ✔ |

"✔" ở các cột GUI/CLI/MCP/MSI nghĩa là **có trong mã nguồn** (đã đọc code). Chưa có nghĩa là
đã build/chạy thành công trên máy này. Kết quả build thực tế xem `docs/BUILD.md` và
`builds/build-report.json`.

## Đặc điểm chung (đã kiểm tra trong source)

- **Ngôn ngữ:** 100% Rust, edition 2024, Cargo resolver 3. Giao diện: egui/eframe (native, không
  Electron/WebView). Mỗi app có thêm bản WebAssembly (`apps/<id>-web`).
- **Giấy phép:** mọi repo là `MIT OR Apache-2.0` (file `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`,
  thường kèm `ATTRIBUTION.md` cho font/icon). Tất cả được giữ nguyên trong `apps/<id>/`.
- **Nhánh mặc định:** `main` (cả 13).
- **Cấu trúc:** mỗi repo là một Cargo workspace riêng: `crates/*` (engine, định dạng file,
  ui-egui, automation/mcp...), `apps/<id>` (GUI), `apps/<id>-cli`, `apps/<id>-web`, `xtask`.
- **Windows:** mỗi app có `packaging/windows/package.ps1` chính thức: build
  `x86_64-pc-windows-msvc` với CRT tĩnh (không cần VC++ redistributable), kiểm tra PE header,
  đóng gói MSI bằng WiX v5 và file zip portable. Hỗ trợ x64, x86, arm64.
- **Hướng dẫn agent của upstream:** cả 13 có `AGENTS.md`; 12/13 có `CLAUDE.md` (trừ PhotoCraft).
  Claude Code tự đọc các file này khi làm việc trong thư mục app.
- **Tình trạng:** dự án đang phát triển rất nhanh (commit nhiều lần mỗi ngày), trạng thái
  "early alpha" theo README của Launcher. Cần đồng bộ upstream thường xuyên.

## Thư viện dùng chung

| Thư viện | Dùng bởi | Cách tham chiếu |
|----------|----------|-----------------|
| `storytold/craft-libs` (craft-ui, craft-print, craft-settings, craft-cms, craft-atomic, craft-quit) | photocraft, vectorcraft, filmcraft, lightcraft, effectcraft, designcraft, soundcraft, cadcraft (và một số crate con) | Cargo git dependency, ghim `rev` (4 rev khác nhau giữa các app) |
| `storytold/filmcraft` (16 crate `filmcraft-*`: codecs, media, av1, hevc, prores...) | effectcraft | git dependency, rev `2c5ba7a726` |
| `storytold/pdfcraft` (`hayro`, `hayro-interpret`, `hayro-syntax`, `hayro-jbig2`) | designcraft | git dependency, rev `b5467d93a0` |
| `storytold/craft-fonts` | 11 app (trừ effectcraft, soundcraft) khi build bản phát hành | Tùy chọn: biến môi trường `CRAFT_FONTS_DIR=<thư mục checkout craft-fonts>` để nhúng font như bản chính thức. Không đặt thì app vẫn build được, chỉ ít font nhúng hơn |

Vì các app ghim **phiên bản khác nhau** của craft-libs và egui, **không** gộp chúng vào một Cargo
workspace chung (sẽ buộc nâng cấp dependency và có thể làm hỏng app). Monorepo giữ 13
workspace độc lập.

## Yêu cầu build trên Windows

| Thành phần | Bắt buộc | Trạng thái máy hiện tại (2026-10-14) |
|------------|----------|--------------------------------------|
| Git (+ `core.longpaths`) | Có | ✔ 2.55 |
| Rust (rustup, toolchain `stable-x86_64-pc-windows-msvc`, ≥ 1.95) | Có | ✘ chưa cài |
| Visual Studio 2022 Build Tools, workload "Desktop development with C++" | Có (linker MSVC) | ✘ chưa cài |
| Windows 10/11 SDK (`rc.exe`, nhúng icon/VERSIONINFO) | Có | ✘ chưa cài |
| .NET SDK + WiX v5.0.2 | Chỉ khi tạo `.msi` | ✘ chưa cài |
| Mạng Internet lần build đầu | Có (Cargo tải crates và các git dependency) | ✔ |

Chạy `scripts\check-env.ps1` để kiểm tra lại bất cứ lúc nào.
