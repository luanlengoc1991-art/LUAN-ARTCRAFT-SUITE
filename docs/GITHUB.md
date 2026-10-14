# Đưa monorepo lên GitHub

Chưa có remote `origin` và chưa push gì. Repo đích do bạn quyết định.

## Lưu ý trước khi tạo repo

- **Kích thước:** khoảng 280 MB mã nguồn (13 app, đã squash lịch sử upstream). Vẫn dưới giới hạn
  khuyến nghị của GitHub, nhưng lần push đầu sẽ lâu.
- **Public hay private:** giấy phép MIT/Apache-2.0 cho phép phân phối lại nếu giữ nguyên file
  LICENSE/NOTICE (đã giữ). Repo private là lựa chọn an toàn nếu bạn có chỉnh sửa riêng chưa muốn
  công khai.
- **GitHub Actions:** `ci.yml` (test app bị thay đổi, Windows runner), `build-windows.yml` (chạy tay),
  `upstream-check.yml` (hằng ngày, mở issue). Với repo private, phút chạy Windows runner bị tính
  gấp đôi vào hạn mức miễn phí. Có thể tắt workflow trong tab Actions nếu không cần.
- Workflow của upstream trong `apps/<app>/.github/` **không** chạy (GitHub chỉ đọc `.github/` ở gốc).

## Các bước (bạn tự chạy, hoặc nhờ Claude sau khi xác nhận tên repo)

1. Tạo repo trống trên GitHub (không tick README/.gitignore/license), ví dụ
   `luanlengoc1991/LUAN-ARTCRAFT-SUITE`.
2. Trong thư mục monorepo:

   ```powershell
   git remote add origin https://github.com/<user>/LUAN-ARTCRAFT-SUITE.git
   git push -u origin main
   ```

3. Tab **Actions** → bật workflow. Chạy thử **Upstream check** bằng "Run workflow".
4. (Tùy chọn) **Settings › Branches** → bảo vệ `main`: yêu cầu PR và yêu cầu CI pass.

Các remote `upstream-<app>` chỉ tồn tại trên máy cục bộ. Script tự thêm lại khi clone ở máy khác.
