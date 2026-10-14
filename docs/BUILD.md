# Build từng ứng dụng (Windows x64)

Cần cài môi trường trước: [WINDOWS-SETUP.md](WINDOWS-SETUP.md).

## Công cụ build thực tế (đã đọc từ source)

Cả 13 app dùng chung một quy trình chính thức, nằm trong `apps/<id>/packaging/windows/package.ps1`:

1. `cargo build --release --locked -p <id> -p <id>-cli --target x86_64-pc-windows-msvc`
   với `RUSTFLAGS=-C target-feature=+crt-static` (CRT tĩnh, không cần VC++ redistributable).
2. Kiểm tra PE header: đúng kiến trúc, GUI app là subsystem Windows, CLI là console.
3. Ký số (`sign.ps1`, bỏ qua khi không có chứng chỉ).
4. WiX v5 → `<id>-<ver>-windows-x64.msi` và `<id>-<ver>-windows-x64-portable.zip`.

Tính năng bật riêng: PhotoCraft `--features heif`, LightCraft `--features lightcraft/heif,lightcraft-cli/heif`
(đều là Rust thuần, không cần thư viện C).

## Lệnh build trong monorepo

```powershell
# một app -> builds\<id>\<version>\windows-x64\<id>.exe, <id>-cli.exe, BUILD-INFO.json
scripts\build-app.ps1 -App pdfcraft

# kèm MSI + zip portable (gọi package.ps1 gốc, cần WiX)
scripts\build-app.ps1 -App pdfcraft -Msi

# bản debug (build nhanh hơn, chạy chậm hơn)
scripts\build-app.ps1 -App pdfcraft -BuildProfile debug

# kiến trúc khác
scripts\build-app.ps1 -App pdfcraft -Arch arm64     # cần MSVC ARM64 build tools

# tất cả 13 app (lỗi một app không dừng các app khác)
scripts\build-all.ps1
scripts\build-all.ps1 -Apps photocraft,wordcraft,gridcraft
```

`build-app.ps1` chạy đúng lệnh `cargo build` của `package.ps1` gốc (cùng package, feature,
`--locked`, CRT tĩnh). Nó **chỉ báo thành công** khi cargo trả về 0, file `.exe` tồn tại và PE
header đúng. `BUILD-INFO.json` ghi lại phiên bản, commit monorepo, commit upstream, rustc,
SHA-256 từng file.

`build-all.ps1` ghi log từng app ở `builds\logs\<id>.log` và báo cáo ở `builds\build-report.json`.

## Cấu trúc đầu ra

```
builds/
  build-report.json
  logs/<id>.log
  <id>/<version>/windows-x64/
    <id>.exe              ứng dụng (GUI)
    <id>-cli.exe          CLI + MCP server
    BUILD-INFO.json
    <id>-<ver>-windows-x64.msi            (khi dùng -Msi)
    <id>-<ver>-windows-x64-portable.zip   (khi dùng -Msi)
```

`builds/` không được commit (đã có trong `.gitignore`).

## Kiểm thử

```powershell
scripts\test-app.ps1 -App gridcraft                 # cargo test (default-members, như CI upstream)
scripts\test-app.ps1 -App gridcraft -Mode check     # chỉ biên dịch, nhanh
scripts\test-app.ps1 -App gridcraft -Mode full      # fmt + clippy -D warnings + test toàn workspace
```

## Build thủ công (không qua script)

```powershell
cd apps\wordcraft
cargo run --release -p wordcraft            # chạy app
cargo run --release -p wordcraft-cli -- --help
pwsh packaging\windows\package.ps1 -Arch x64
```

## Lỗi thường gặp

| Triệu chứng | Nguyên nhân | Cách xử lý |
|-------------|-------------|------------|
| `linker 'link.exe' not found` | Chưa cài VS Build Tools (C++) | WINDOWS-SETUP.md bước 2 |
| `package ... requires rustc 1.95` | Rust cũ | `rustup update stable` |
| `the lock file needs to be updated but --locked was passed` | Cargo.lock của app đã bị sửa | Không chạy `cargo update`. Khôi phục Cargo.lock của app đó từ commit trước |
| Lỗi đường dẫn quá dài | Thư mục sâu + tên dài | `$env:CARGO_TARGET_DIR='D:\ct\<app>'` |
| `failed to fetch https://github.com/storytold/craft-libs` | Mất mạng hoặc GitHub bị chặn | Kiểm tra mạng. Cargo cần tải git dependency lần đầu |
| `rc.exe not found` / icon không nhúng | Thiếu Windows SDK | Cài lại Build Tools với `--includeRecommended` |
