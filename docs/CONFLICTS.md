# Giải quyết xung đột khi cập nhật upstream

Xung đột xảy ra khi bạn và nhà phát triển cùng sửa một đoạn code. `scripts\upstream-update.ps1`
dừng lại và **không** tự chọn bên nào.

## Trạng thái khi dừng

- Bạn đang ở nhánh `upstream-sync/<id>-<thời gian>`. `main` an toàn.
- Có tag `pre-sync/<id>-<thời gian>` tại điểm trước khi sync.
- Merge đang dở. Xem danh sách file: `git status` hoặc `git diff --name-only --diff-filter=U`.

## Xử lý trong Cursor

1. Mở tab Source Control. Các file xung đột nằm trong "Merge Changes".
2. Trong mỗi file, Cursor hiện các khối:
   ```
   <<<<<<< HEAD            ← bản của bạn (monorepo)
   ...
   =======
   ...
   >>>>>>> <sha>           ← bản upstream mới
   ```
   Chọn **Accept Current** (giữ của bạn), **Accept Incoming** (lấy upstream), **Accept Both**, hoặc
   sửa tay để kết hợp.
3. Có thể nhờ Claude Code: *"Giải quyết xung đột trong apps/photocraft/... giữ chỉnh sửa của tôi
   về X, lấy phần còn lại từ upstream, giải thích từng quyết định"*. Xem lại diff trước khi commit.
4. Khi xong:
   ```powershell
   git add <các file đã xử lý>
   git commit                                   # hoàn tất merge
   scripts\test-app.ps1 -App <id>
   ```
5. Cập nhật thủ công `config/apps.json` → `upstream.importedCommit` của app = SHA upstream mới
   (script đã in ra), rồi commit `chore(<id>): record upstream commit ...`.
6. Merge vào main như bình thường (UPSTREAM-SYNC.md bước 4).

## Các trường hợp đặc biệt

| Trường hợp | Cách xử lý |
|------------|------------|
| `Cargo.lock` xung đột | Thường lấy bản upstream (Accept Incoming), rồi `cargo check --locked` trong app. Nếu bạn đã thêm dependency, chạy `cargo check` (không `--locked`) để Cargo bổ sung, rồi kiểm tra diff của Cargo.lock chỉ thêm đúng thứ bạn cần |
| Upstream xóa file mà bạn đã sửa (`deleted by them`) | Tìm xem code được chuyển đi đâu (`git log` trên upstream). Áp lại chỉnh sửa vào vị trí mới, rồi `git rm` file cũ |
| Upstream đổi tên/tách module | Tương tự: áp lại chỉnh sửa của bạn vào cấu trúc mới, đừng giữ file cũ song song |
| Quá nhiều xung đột | Bỏ lần sync này: `git merge --abort` rồi `git switch main`. Cân nhắc tách chỉnh sửa riêng thành file/module mới để lần sau ít xung đột hơn |
| File giấy phép (`LICENSE-*`, `NOTICE`, `ATTRIBUTION.md`) | Luôn lấy bản upstream |

## Những lệnh KHÔNG dùng để "thoát" xung đột

`git reset --hard`, `git checkout -- .`, `git clean -fd`, `git push --force`. Chúng có thể xóa
chỉnh sửa của bạn. `git merge --abort` là cách an toàn để bỏ một lần merge dở.
