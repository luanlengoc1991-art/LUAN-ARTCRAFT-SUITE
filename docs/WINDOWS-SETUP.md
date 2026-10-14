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
- **Dung lượng (đo thực tế 2026-10-14):** mỗi app build release tạo 1,2–2,5 GB trong
  `apps\<app>\target\`. 8 app tổng khoảng 15 GB. Có thể xóa `apps\<app>\target` để giải phóng
  (chỉ là đầu ra build, build lại được).
- **Thời gian (đo thực tế trên máy này):** lần build đầu mỗi app mất 2–6,5 phút (GridCraft 108 s,
  VectorCraft 385 s). Cả 8 app khoảng 35 phút. Các lần sau nhanh hơn nhiều.
- Sau khi cài Build Tools, trình cài khuyên khởi động lại máy. Trên máy này build vẫn chạy được
  ngay mà chưa cần khởi động lại.
- **Windows Defender** có thể làm build chậm. Có thể thêm `apps\*\target` và `%USERPROFILE%\.cargo`
  vào danh sách loại trừ (tùy bạn quyết định).
