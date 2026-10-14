# Dùng monorepo trong Cursor + Claude Code

## Mở dự án

Cursor → **File › Open Folder** → chọn
`D:\CLAUDE CURSO WEBSITE LUAN\APP ADOBE  ARTCRAFT LUAN DESIGNER\LUAN-ARTCRAFT-SUITE`.

Chỉ cần mở một thư mục này. Cài các extension được gợi ý (Cursor hỏi tự động, hoặc xem
`.vscode/extensions.json`): rust-analyzer, Even Better TOML, PowerShell, Claude Code.

## rust-analyzer: chọn app đang làm

Repo có 13 Cargo workspace độc lập. Nạp cả 13 cùng lúc tốn rất nhiều RAM, nên mặc định
rust-analyzer chưa nạp app nào. Chọn app:

```powershell
scripts\focus-app.ps1 -App photocraft              # một app
scripts\focus-app.ps1 -App effectcraft,filmcraft   # vài app
scripts\focus-app.ps1 -All                         # cả 13 (máy nhiều RAM)
```

Sau đó Ctrl+Shift+P → **Developer: Reload Window**.

## Claude Code tự biết gì

- `CLAUDE.md` ở gốc: kiến trúc, quy tắc phạm vi sửa, lệnh test/build, quy tắc bảo vệ mã nguồn.
- `apps/<id>/AGENTS.md` và `apps/<id>/CLAUDE.md`: quy tắc riêng của nhà phát triển từng app.
  Claude Code tự nạp khi làm việc trong thư mục đó.
- `.cursor/rules/monorepo.mdc`: cùng quy tắc cho agent tích hợp của Cursor.

## Mẫu yêu cầu cho Claude Code

| Mục đích | Câu lệnh mẫu |
|----------|--------------|
| Phân tích | "Giải thích kiến trúc engine của PdfCraft, các crate chính và luồng mở một file PDF." |
| Sửa một app | "Trong **WordCraft**, thêm phím tắt Ctrl+Alt+W để đếm từ. Chỉ sửa wordcraft, chạy test." |
| Nhiều app | "Thêm tùy chọn X cho **GridCraft và DeckCraft**. Liệt kê kế hoạch cho từng app trước khi sửa." |
| Tìm code chung | "Tìm cách các app triển khai `control_server.rs`, so sánh điểm giống và khác." |
| Ảnh hưởng | "Chạy scripts\affected.ps1 và cho biết thay đổi hiện tại ảnh hưởng app nào." |
| Test | "Chạy scripts\test-app.ps1 -App lightcraft -Mode check và sửa lỗi biên dịch nếu có." |
| Build | "Build PhotoCraft bằng scripts\build-app.ps1 và báo đường dẫn file .exe." |
| Cập nhật | "Chạy upstream-update -DryRun cho FilmCraft và tóm tắt thay đổi upstream." |

Nêu rõ tên app trong yêu cầu. Theo `CLAUDE.md`, Claude không được sửa app bạn không nhắc tới.

## Quy trình làm việc hằng ngày

```powershell
git switch -c feat/wordcraft-word-count       # nhánh riêng cho thay đổi
# ... nhờ Claude sửa apps/wordcraft ...
scripts\affected.ps1                           # phải chỉ ra: wordcraft
scripts\test-app.ps1 -App wordcraft
scripts\build-app.ps1 -App wordcraft
git add apps/wordcraft; git commit -m "feat(wordcraft): word count shortcut"
git switch main; git merge --no-ff feat/wordcraft-word-count
```

## MCP trong Cursor / Claude Code

Sau khi build app, xem [MCP-AUTOMATION.md](MCP-AUTOMATION.md) và `config/mcp.example.json`.
