# Cài đặt môi trường build trên Windows 11 x64

Làm một lần. Tất cả lệnh chạy trong PowerShell. Bước 2 cần quyền Administrator (UAC).

## 1. Kiểm tra máy

```powershell
cd "D:\CLAUDE CURSO WEBSITE LUAN\APP ADOBE  ARTCRAFT LUAN DESIGNER\LUAN-ARTCRAFT-SUITE"
powershell -ExecutionPolicy Bypass -File scripts\check-env.ps1
```

Script chỉ đọc, không cài gì. Nó in ra mục còn thiếu kèm lệnh cài.

## 2. Visual Studio 2022 Build Tools (linker MSVC + Windows SDK)

Khoảng 6–8 GB. Rust trên Windows cần `link.exe` và `rc.exe` từ gói này.

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

`--includeRecommended` kèm Windows 11 SDK (`rc.exe`, `signtool.exe`).

## 3. Rust

```powershell
winget install --id Rustlang.Rustup -e
# mở PowerShell MỚI rồi:
rustup default stable-x86_64-pc-windows-msvc
rustup update stable
rustc --version          # phải >= 1.95
```

Các app không ghim toolchain (không có `rust-toolchain.toml`). Bản `stable` mới nhất là đủ.

## 4. (Tùy chọn) Tạo file cài đặt .msi

Chỉ cần khi muốn có `.msi` giống bản phát hành chính thức. File `.exe` không cần bước này.

```powershell
winget install --id Microsoft.DotNet.SDK.8 -e
# PowerShell mới:
dotnet tool install --global wix --version 5.0.2
wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
```

## 5. (Tùy chọn) Font giống bản chính thức

```powershell
git clone https://github.com/storytold/craft-fonts D:\craft-fonts
$env:CRAFT_FONTS_DIR = 'D:\craft-fonts'      # đặt trước khi build
```

## 6. Kiểm tra lại

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-env.ps1   # phải in "Environment ready."
```

## Lưu ý

- **Đường dẫn dài / dấu cách:** thư mục dự án có dấu cách và tiếng Việt. Git đã bật
  `core.longpaths`. Nếu gặp lỗi đường dẫn quá dài khi build, đặt thư mục target ngắn:
  `$env:CARGO_TARGET_DIR = 'D:\ct\<app>'` (mỗi app một thư mục riêng).
- **Dung lượng:** mỗi app build release tạo khoảng 2–6 GB trong `apps\<app>\target\`. Cả 13 app
  cần khoảng 40–70 GB. Ổ D: hiện còn trống khoảng 488 GB.
- **Thời gian:** lần build đầu mỗi app mất khoảng 5–20 phút tùy CPU (Cargo tải và biên dịch
  toàn bộ dependency). Các lần sau nhanh hơn nhiều.
- **Windows Defender** có thể làm build chậm. Có thể thêm `apps\*\target` và `%USERPROFILE%\.cargo`
  vào danh sách loại trừ (tùy bạn quyết định).
