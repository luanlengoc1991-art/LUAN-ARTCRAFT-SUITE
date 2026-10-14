# Độc lập khỏi nhà phát triển gốc

Mục tiêu: mọi thứ cần để **sửa, build, đóng gói, phát hành và cài đặt** đều nằm trên GitHub của bạn
(`luanlengoc1991-art`). Nhà phát triển gốc (`storytold`, getartcraft.com, api.storyteller.ai) có xóa
hay ngừng hoạt động thì bạn vẫn làm việc bình thường.

## Cái gì nằm ở đâu

| Thành phần | Vị trí trên GitHub của bạn |
|------------|----------------------------|
| Mã nguồn 13 app + trình quản lý | `LUAN-ARTCRAFT-SUITE/apps/<app>/` |
| Thư viện git mà app ghim (craft-libs, winprint.rs, các crate filmcraft/pdfcraft, winit fork) | `LUAN-ARTCRAFT-SUITE/vendor/git/` + `.cargo/config.toml` |
| Script build, test, đóng gói, phát hành, cài máy mới | `LUAN-ARTCRAFT-SUITE/scripts/` |
| File cài đặt `.msi` / `.zip` của từng app | Releases của `luanlengoc1991-art/<app>` |
| Trình quản lý ArtCraft Launcher (`.msi`) | Releases của `luanlengoc1991-art/craft-launcher` |
| Ảnh minh họa launcher hiển thị | `luanlengoc1991-art/craft-launcher-assets` (tag `v1`) |

Còn tải từ Internet khi build: các gói **crates.io**. Đây là kho thư viện chung của cộng đồng
Rust, không thuộc nhà phát triển ArtCraft. Gói bị "yank" vẫn tải được theo `Cargo.lock`; crates.io
chỉ cho tác giả xóa hẳn gói trong một số trường hợp hẹp (gói mới, ít người dùng).

Đã kiểm chứng ngày 2026-10-14: với một `CARGO_HOME` trống (giống máy mới), DesignCraft,
PhotoCraft và EffectCraft tải **0** repo git, chỉ tải từ crates.io.

## Cách hoạt động

- **Vendor:** `scripts\vendor-git-deps.ps1` chép đúng bản (đúng `rev`) của từng thư viện git vào
  `vendor/git/<repo>-<rev>/` và sinh `.cargo/config.toml` ở gốc. Cargo đọc file config ở thư mục cha,
  nên mọi app trong `apps/` tự dùng bản trong repo mà **không sửa file nào của app**.
- **Khi nâng cấp:** `scripts\upgrade.ps1` tự vendor thêm nếu bản upstream mới ghim `rev` khác.
- **Sửa thư viện dùng chung:** file trong `vendor/` có checksum (`.cargo-checksum.json`), nên không
  sửa trực tiếp. Muốn sửa craft-libs thì nhờ Claude chuyển thư viện đó thành mã nguồn riêng trong
  repo (ví dụ `libs/craft-libs`) và trỏ app tới bằng `[patch]`.

## Trình quản lý (ArtCraft Launcher của bạn)

`apps/craft-launcher` là launcher gốc, đã sửa để:
- cài và cập nhật 13 app từ `luanlengoc1991-art/<app>` (Releases, file `.msi` ưu tiên, `.zip` dự phòng,
  kiểm tra SHA-256);
- tự cập nhật chính nó từ `luanlengoc1991-art/craft-launcher`;
- **không** gọi máy chủ `api.storyteller.ai` (feed bị tắt), chỉ hỏi GitHub;
- link "GitHub"/"Website" trỏ về monorepo của bạn.

Còn giữ: link Discord và mạng xã hội của cộng đồng ArtCraft ở chân trang. Đây chỉ là link, không
ảnh hưởng chức năng; muốn bỏ thì nhờ Claude sửa `SOCIAL`/`DISCORD_URL` trong
`apps/craft-launcher/crates/core/src/catalog.rs`.

Launcher chỉ hiện "có bản cài" cho app nào đã được phát hành lên repo tương ứng. Hiện bộ build
mặc định có 8 app + launcher. 5 app còn lại sẽ hiện "chưa có bản phát hành" cho tới khi bạn build
và phát hành chúng.

## Phát hành tự động: sửa → push → mọi máy tự cập nhật

```
bạn sửa apps/<app>/ → git push
   → GitHub Actions "Release" (.github/workflows/release.yml), chỉ chạy cho app bị sửa:
       tính phiên bản mới (scripts/release-version.ps1: luôn cao hơn bản trước, ví dụ 0.9.0 → 0.9.1)
       đặt phiên bản vào mã (cargo xtask version set, chỉ trong lúc build, không commit)
       build .exe + .msi bằng công cụ đóng gói gốc, tạo Release trên luanlengoc1991-art/<app>
   → ArtCraft Launcher trên mọi máy hỏi GitHub 20 phút/lần, tự tải và cài bản mới
       (tự cập nhật bật mặc định; tắt được trong Settings)
   → <app>-cli.exe (MCP) nằm trong cùng file .msi nên cập nhật cùng lúc, cùng đường dẫn
```

- Sửa `apps/craft-launcher` thì chính **trình quản lý** cũng được phát hành bản mới và tự cập nhật.
- `scripts\upgrade.ps1` (nâng cấp từ upstream) push lên `main`, nên các app nâng cấp cũng tự phát hành.
- Chỉ các app có `build.buildByDefault: true` trong `config/apps.json` mới tự phát hành (8 app +
  trình quản lý). Muốn thêm app nào, đổi giá trị đó thành `true`.
- Một lần build trên GitHub mất khoảng 15–30 phút mỗi app (repo public nên miễn phí). Theo dõi ở
  tab **Actions** → **Release**.
- Phát hành tay một app bất kỳ: tab Actions → Release → **Run workflow** → nhập tên app (hoặc `all`).
- Secret `RELEASE_TOKEN` (Settings → Secrets → Actions) là token GitHub CLI của tài khoản bạn. Nếu
  bạn đăng xuất `gh` hoặc thu hồi token, hãy tạo token mới (quyền `repo`) và cập nhật secret này.

Phát hành từ máy cục bộ (không qua GitHub Actions) vẫn dùng được:

```powershell
scripts\build-app.ps1 -App pdfcraft -Msi
scripts\publish-release.ps1 -Apps pdfcraft
```

## MCP cho Claude Code và Cursor

Mỗi app cài qua launcher đều có MCP server: `C:\Program Files\<App>\<app>-cli.exe mcp`. Đăng ký
một lần trên mỗi máy (chạy lại khi cài thêm app):

```powershell
scripts\register-mcp.ps1                        # hoặc: -Folder D:\ArtCraftWork để giới hạn thư mục
```

Script ghi `.mcp.json` (Claude Code trong dự án này) và `%USERPROFILE%\.cursor\mcp.json` (Cursor,
mọi dự án). Khi launcher cập nhật app, file `.exe` được thay tại chỗ nên MCP tự lên bản mới.
Trình quản lý không có MCP riêng. Đề xuất "MCP Gateway" gom 13 app vào một server nằm trong
`docs/MCP-AUTOMATION.md`.

Bản cài chưa được ký số (không có chứng chỉ code-signing), nên Windows SmartScreen có thể cảnh báo
"Unknown publisher" lần đầu. Chọn **More info → Run anyway**.

## Sang máy khác

```powershell
winget install --id Git.Git -e
git clone -c core.longpaths=true https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE.git
cd LUAN-ARTCRAFT-SUITE
powershell -ExecutionPolicy Bypass -File scripts\setup-machine.ps1
```

Script cài Rust, VS Build Tools, .NET + WiX, GitHub CLI qua winget rồi kiểm tra môi trường. Sau đó
mọi lệnh trong README dùng được ngay. Chạy `gh auth login` nếu cần phát hành.
