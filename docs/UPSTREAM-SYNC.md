# Theo dõi và cập nhật từ upstream

## Nguyên lý

- Mỗi app được nhập bằng `git subtree add --squash` từ `https://github.com/storytold/<id>` nhánh
  `main`. Lịch sử upstream được nén thành một commit "Squashed 'apps/<id>/' content from commit
  <sha>", và commit đó ghi metadata `git-subtree-dir` / `git-subtree-split` để lần pull sau biết
  điểm gốc.
- Commit upstream đang dùng được ghi trong `config/apps.json` → `apps[].upstream.importedCommit`.
- Khi cập nhật, `git subtree pull --squash` thực hiện **merge 3 chiều**: bản gốc cũ, bản upstream
  mới và bản của bạn. Chỉnh sửa của bạn được giữ lại. Chỗ nào cả hai bên cùng sửa sẽ thành xung
  đột và script **dừng lại** để bạn xem xét.
- Các remote `upstream-<id>` được thêm tự động khi cần (bản clone mới của monorepo không mang
  theo remote).

## 1. Kiểm tra có bản mới không (chỉ đọc)

```powershell
scripts\upstream-status.ps1
```

```
app         state            imported   upstream   localCommits
photocraft  UPDATE AVAILABLE 7250e6e29a c699e3cda6            0
vectorcraft up-to-date       0a33cff3f6 0a33cff3f6            0
...
```

`localCommits` là số commit chỉnh sửa riêng của bạn trên app đó.

GitHub Actions `upstream-check.yml` chạy kiểm tra này **hàng ngày** (khi repo đã lên GitHub) và mở
hoặc cập nhật một issue "Upstream updates available". Nó **không bao giờ** tự merge.

## 2. Xem trước một lần cập nhật

```powershell
scripts\upstream-update.ps1 -App photocraft -DryRun
```

Script in ra các commit upstream mới, các file bạn đã sửa, và **các file cả hai bên cùng sửa**
(khả năng xung đột cao). Không thay đổi gì.

## 3. Cập nhật

```powershell
scripts\upstream-update.ps1 -App photocraft
```

Script sẽ:

1. Từ chối chạy nếu còn thay đổi chưa commit. Nó không stash, không reset.
2. Tạo tag `pre-sync/photocraft-<thời gian>` tại commit hiện tại làm điểm khôi phục.
3. Tạo nhánh mới `upstream-sync/photocraft-<thời gian>`. Nhánh `main` không bị đụng tới.
4. Chạy `git subtree pull --prefix=apps/photocraft upstream-photocraft main --squash`.
5. Nếu sạch: cập nhật `config/apps.json` (commit, version, rust-version) và commit.
6. Nếu xung đột: **dừng**, liệt kê file xung đột, giữ trạng thái merge để bạn xử lý. Xem
   [CONFLICTS.md](CONFLICTS.md).

## 4. Kiểm thử rồi mới đưa vào main

```powershell
scripts\test-app.ps1 -App photocraft
scripts\build-app.ps1 -App photocraft
git switch main
git merge --no-ff upstream-sync/photocraft-<thời gian>
```

Mỗi lần chỉ cập nhật một app, để dễ khoanh vùng lỗi.

## Nâng cấp liên tục bằng một lệnh

```powershell
scripts\upgrade.ps1            # mọi app có bản mới, rồi push lên GitHub
scripts\upgrade.ps1 -NoPush    # không đẩy lên GitHub
scripts\upgrade.ps1 -CheckAll  # kiểm tra biên dịch cả 13 app (mặc định chỉ 8 app trong bộ build)
```

Với từng app có bản mới: chạy `upstream-update.ps1` (nhánh riêng, tag khôi phục). Nếu **xung đột
thì dừng toàn bộ** để bạn xem. Sau đó kiểm tra biên dịch; lỗi thì không merge và giữ nhánh để xem.
Đạt thì merge `--no-ff` vào `main`. Cuối cùng `git push origin main` (không bao giờ force-push).

## 5. Quay lại nếu cần (không mất dữ liệu)

- Bỏ nhánh sync chưa merge: `git switch main` (nhánh sync vẫn còn để xem lại, xóa khi chắc chắn:
  `git branch -D upstream-sync/...`).
- Đã merge vào main mà muốn hoàn tác: `git revert -m 1 <merge-commit>` (tạo commit đảo ngược,
  không viết lại lịch sử).

## Quy tắc giữ chỉnh sửa riêng dễ đồng bộ

- Sửa ít và tập trung. Ưu tiên thêm file/module mới thay vì viết lại file của upstream.
- Không format lại toàn bộ file, không đổi tên hàng loạt.
- Commit chỉnh sửa riêng tách biệt, có tiền tố `feat(<id>):` / `fix(<id>):`.
- Không sửa `Cargo.lock` của app trừ khi bắt buộc (dễ xung đột nhất).
- Ghi lý do chỉnh sửa lớn vào `docs/CUSTOMIZATIONS.md` (tạo khi có chỉnh sửa đầu tiên) để lần
  đồng bộ sau biết phải giữ gì.

## Ghi chú về EffectCraft và DesignCraft

EffectCraft ghim crate của FilmCraft, DesignCraft ghim crate của PdfCraft theo **git rev** trên
GitHub. Khi đồng bộ FilmCraft/PdfCraft, hai app kia **không** tự đổi. Chúng chỉ đổi khi upstream của
chính chúng nâng rev.
