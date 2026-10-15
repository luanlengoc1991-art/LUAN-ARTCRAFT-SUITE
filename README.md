# LUAN-ARTCRAFT-SUITE

Monorepo chứa mã nguồn đầy đủ của **13 ứng dụng ArtCraft** (https://getartcraft.com/apps), nhập
từ repository chính thức `github.com/storytold/<app>` bằng **git subtree**. Bạn quản lý, chỉnh
sửa, build và cập nhật cả 13 app từ một project duy nhất trong Cursor + Claude Code.

> Đây là bản tổng hợp cá nhân, không phải dự án chính thức của ArtCraft. Mỗi app giữ nguyên giấy
> phép gốc **MIT OR Apache-2.0** và thông báo bản quyền trong `apps/<app>/LICENSE-*`, `NOTICE`,
> `ATTRIBUTION.md`. Bản quyền thuộc ArtCraft Team và các contributor.

GitHub: https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE

## 13 ứng dụng

| App | Thư mục | Chức năng | Upstream |
|-----|---------|-----------|----------|
| PhotoCraft | [apps/photocraft](apps/photocraft) | Chỉnh sửa ảnh | [storytold/photocraft](https://github.com/storytold/photocraft) |
| VectorCraft | [apps/vectorcraft](apps/vectorcraft) | Minh họa vector | [storytold/vectorcraft](https://github.com/storytold/vectorcraft) |
| FilmCraft | [apps/filmcraft](apps/filmcraft) | Dựng video | [storytold/filmcraft](https://github.com/storytold/filmcraft) |
| LightCraft | [apps/lightcraft](apps/lightcraft) | Thư viện ảnh, xử lý RAW | [storytold/lightcraft](https://github.com/storytold/lightcraft) |
| PdfCraft | [apps/pdfcraft](apps/pdfcraft) | Đọc, sắp xếp, bảo mật PDF | [storytold/pdfcraft](https://github.com/storytold/pdfcraft) |
| EffectCraft | [apps/effectcraft](apps/effectcraft) | Motion graphics, VFX | [storytold/effectcraft](https://github.com/storytold/effectcraft) |
| DesignCraft | [apps/designcraft](apps/designcraft) | Dàn trang, xuất bản | [storytold/designcraft](https://github.com/storytold/designcraft) |
| FigCraft | [apps/figcraft](apps/figcraft) | Thiết kế giao diện, prototype | [storytold/figcraft](https://github.com/storytold/figcraft) |
| WordCraft | [apps/wordcraft](apps/wordcraft) | Soạn thảo văn bản | [storytold/wordcraft](https://github.com/storytold/wordcraft) |
| SoundCraft | [apps/soundcraft](apps/soundcraft) | Thu âm, chỉnh sửa, mix | [storytold/soundcraft](https://github.com/storytold/soundcraft) |
| DeckCraft | [apps/deckcraft](apps/deckcraft) | Trình chiếu | [storytold/deckcraft](https://github.com/storytold/deckcraft) |
| CADCraft | [apps/cadcraft](apps/cadcraft) | CAD, bản vẽ kỹ thuật | [storytold/cadcraft](https://github.com/storytold/cadcraft) |
| GridCraft | [apps/gridcraft](apps/gridcraft) | Bảng tính | [storytold/gridcraft](https://github.com/storytold/gridcraft) |
| **ArtCraft Launcher** | [apps/craft-launcher](apps/craft-launcher) | **Trình quản lý**: cài và cập nhật các app từ GitHub của bạn | [storytold/craft-launcher](https://github.com/storytold/craft-launcher) (đã tùy biến) |

**Độc lập:** mọi thứ cần để build, phát hành và cài đặt đều nằm trên GitHub của bạn: mã nguồn,
thư viện (`vendor/`), file cài (Releases của `luanlengoc1991-art/<app>`) và trình quản lý. Xem
[docs/INDEPENDENCE.md](docs/INDEPENDENCE.md).

**Tải trình quản lý (link cố định, luôn là bản mới nhất):**
- Bộ cài: https://github.com/luanlengoc1991-art/craft-launcher/releases/latest/download/ArtCraft-Launcher-Setup.msi
- Bản chạy ngay không cần cài: https://github.com/luanlengoc1991-art/craft-launcher/releases/latest/download/ArtCraft-Launcher.exe

Cài trình quản lý, mở lên, rồi bấm **Install** cho app bạn cần. Sau đó app tự cập nhật mỗi khi bạn
push thay đổi lên repo này.

**Quy trình: mọi máy giống nhau, chỉ cần cài trình quản lý.**

1. Trên máy mới, chỉ cần cài ArtCraft Launcher, các bước sau diễn ra tự động.
2. Launcher đọc danh sách [config/suite-sync.json](config/suite-sync.json) trên GitHub. App nào
   trong danh sách mà máy chưa có thì launcher tự cài, cùng với ArtCraft MCP. MCP được đăng ký sẵn
   vào Claude Code và Cursor.
3. Khi bạn push thay đổi lên `main`, GitHub build và phát hành bản mới. Launcher trên mọi máy thấy
   bản mới trong vòng khoảng 20 phút, gửi thông báo và tự cập nhật. Khi xong sẽ có thêm một thông
   báo "… updated to …". Tab **Updates** trong launcher có nút cập nhật tay, còn thanh **ArtCraft
   MCP** ở đầu danh sách app có nút cập nhật MCP.
4. Muốn thêm hoặc bớt app trên mọi máy: sửa `config/suite-sync.json` rồi push. Gỡ app khỏi danh
   sách **không** xóa app ở máy nào. Muốn một máy không tự cài theo danh sách: vào Preferences, bỏ
   chọn "Keep the same apps as my other computers".
5. Để làm việc, dùng Claude Code hoặc Cursor ra lệnh qua MCP `artcraft`. Đây là một MCP duy nhất
   và gọi được tool của từng app.

App cài vào Program Files nên Windows hỏi quyền (UAC) mỗi lần cài hoặc cập nhật app. Bấm **Yes**
là được. ArtCraft MCP cài theo user nên không hỏi.

**MCP tổng (một MCP cho mọi app đã cài, dùng trong Claude Code và Cursor):**
- Tải: https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/latest/download/ArtCraft-MCP.exe
- Cài và đăng ký vào Claude Code + Cursor bằng một lệnh PowerShell:
  `irm https://raw.githubusercontent.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/main/scripts/install-mcp.ps1 | iex`

Chi tiết: [tools/artcraft-mcp/README.md](tools/artcraft-mcp/README.md).

Tất cả viết bằng Rust thuần (egui). Mỗi app có GUI (`<app>.exe`), CLI kèm **MCP server**
(`<app>-cli.exe mcp`) và bản web. Chi tiết: [docs/AUDIT.md](docs/AUDIT.md).

## Cấu trúc

```
LUAN-ARTCRAFT-SUITE/
├─ apps/<app>/            mã nguồn đầy đủ từng app (Cargo workspace độc lập, giữ nguyên cấu trúc gốc)
├─ config/apps.json       manifest: upstream, commit, phiên bản, giấy phép, cách build
├─ config/mcp.example.json  cấu hình MCP mẫu cho Claude Code / Cursor
├─ tools/artcraft-mcp/    MCP tổng: một MCP server cho cả 13 app (không sửa app nào)
├─ scripts/               build, test, phân tích ảnh hưởng, đồng bộ upstream (PowerShell)
├─ docs/                  tài liệu
├─ builds/                đầu ra build cục bộ (không commit)
├─ .github/workflows/     CI, build Windows, kiểm tra upstream hằng ngày
├─ CLAUDE.md              hướng dẫn cho Claude Code
└─ .cursor/rules/         quy tắc cho agent của Cursor
```

## Bắt đầu nhanh (Windows)

```powershell
scripts\check-env.ps1                    # 1. kiểm tra Rust, MSVC, SDK, WiX
#    cài phần còn thiếu: docs/WINDOWS-SETUP.md
scripts\build-app.ps1 -App pdfcraft      # 2. build một app -> builds\pdfcraft\<ver>\windows-x64\
scripts\build-all.ps1                    # 3. build cả 13 (báo cáo builds\build-report.json)
```

Nếu PowerShell chặn script: `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` (một lần), hoặc
chạy `powershell -ExecutionPolicy Bypass -File scripts\<script>.ps1 ...`.

## Lệnh thường dùng

| Việc | Lệnh |
|------|------|
| Kiểm tra môi trường | `scripts\check-env.ps1` |
| Chọn app cho rust-analyzer | `scripts\focus-app.ps1 -App <app>` |
| App nào bị ảnh hưởng | `scripts\affected.ps1` |
| Test một app | `scripts\test-app.ps1 -App <app> [-Mode check\|test\|full]` |
| Build một app | `scripts\build-app.ps1 -App <app> [-Msi] [-Arch x64]` |
| Build bộ mặc định (8 app) | `scripts\build-all.ps1 [-Apps a,b]` |
| Kiểm tra bản mới upstream | `scripts\upstream-status.ps1` |
| Cập nhật một app từ upstream | `scripts\upstream-update.ps1 -App <app> [-DryRun]` |
| **Nâng cấp liên tục** (sync → kiểm tra → merge → push GitHub) | `scripts\upgrade.ps1` |
| Phát hành file cài lên GitHub (cho trình quản lý) | `scripts\publish-release.ps1 [-Apps a,b]` |
| Vendor lại thư viện git (sau khi đổi rev) | `scripts\vendor-git-deps.ps1` |
| Cài đặt máy mới | `scripts\setup-machine.ps1` |
| Cài/cập nhật MCP tổng + đăng ký Claude Code, Cursor | `scripts\install-mcp.ps1 [-SearchDir D:\Apps] [-RemoveAppEntries]` |

## Tài liệu

- [docs/STATUS.md](docs/STATUS.md): trạng thái build, MCP, đồng bộ (báo cáo mới nhất)
- [docs/AUDIT.md](docs/AUDIT.md): kiểm tra mã nguồn 13 app, thư viện dùng chung, yêu cầu build
- [docs/WINDOWS-SETUP.md](docs/WINDOWS-SETUP.md): cài đặt môi trường Windows
- [docs/BUILD.md](docs/BUILD.md): build từng app, đầu ra, lỗi thường gặp
- [docs/MCP-AUTOMATION.md](docs/MCP-AUTOMATION.md): MCP/CLI từng app, kết nối Claude Code, MCP tổng
- [docs/UPSTREAM-SYNC.md](docs/UPSTREAM-SYNC.md): theo dõi và cập nhật upstream
- [docs/CONFLICTS.md](docs/CONFLICTS.md): giải quyết xung đột
- [docs/CURSOR.md](docs/CURSOR.md): làm việc trong Cursor + Claude Code
- [docs/GITHUB.md](docs/GITHUB.md): đưa monorepo lên GitHub

## Bảo vệ mã nguồn

- Không dùng submodule. Mỗi app là một subtree, có thể đồng bộ lại với upstream bất cứ lúc nào.
- Cập nhật upstream luôn chạy trên nhánh riêng, có tag khôi phục, **dừng khi xung đột**, không tự
  merge vào `main`.
- Chỉnh sửa của bạn được giữ nhờ merge 3 chiều. Không có lệnh nào trong `scripts/` dùng
  `reset --hard`, `clean` hay force-push.
