# ArtCraft MCP: một MCP server cho mọi app ArtCraft

`artcraft-mcp.exe` là **một** MCP server (stdio). Bạn đăng ký nó một lần trong Claude Code hoặc
Cursor, và nó điều khiển được mọi app ArtCraft đã cài trên máy. Chương trình **không sửa app nào**:
mỗi app vẫn dùng MCP server riêng của mình (`<id>-cli.exe mcp`). Gateway chỉ khởi động server
của app khi cần, rồi chuyển tiếp lệnh tới đó.

## Tải và cài

**Cách đơn giản nhất: không cần làm gì.** ArtCraft Launcher tự cài, cập nhật và đăng ký MCP này vào
Claude Code và Cursor. Thanh **ArtCraft MCP** ở đầu cửa sổ launcher cho biết phiên bản đang cài,
và ô Claude Code / Cursor sáng xanh khi MCP đã được đăng ký vào đó. Cách cài tay bên dưới chỉ dùng
khi máy chưa có launcher.

- Link cố định, luôn là bản mới nhất:
  https://github.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/releases/latest/download/ArtCraft-MCP.exe
- Cài và đăng ký vào Claude Code (mọi project) và Cursor bằng một lệnh PowerShell:

  ```powershell
  irm https://raw.githubusercontent.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/main/scripts/install-mcp.ps1 | iex
  ```

  Nếu đã có repo trên máy: `scripts\install-mcp.ps1 [-SearchDir D:\Apps] [-Folder D:\Work] [-RemoveAppEntries]`.
  Chạy lại lệnh này để cập nhật lên bản mới.

Sau khi cài, nạp lại MCP: trong Claude Code gõ `/mcp` hoặc reload cửa sổ, trong Cursor vào Settings > MCP.

## Dùng thế nào

Tool của từng app có tên dạng `<app>_<tool>`, ví dụ `vectorcraft_draw_shape`, `gridcraft_list_commands`
hay `photocraft_command_run`. Bạn chỉ cần ra lệnh bằng lời ("vẽ ngôi sao cam trong VectorCraft"),
Claude sẽ tự chọn tool phù hợp.

| Tool | Việc |
|------|------|
| `artcraft_apps` | App nào đã cài, đường dẫn, cửa sổ có đang mở không, số tool |
| `artcraft_open` | Mở cửa sổ app, hoặc gắn vào cửa sổ đang mở, để các lệnh sau thao tác **trực tiếp trên cửa sổ** |
| `artcraft_call` | Gọi bất kỳ tool nào bằng tên gốc của app: `{app, tool, arguments}` |
| `artcraft_tools` | (chỉ ở chế độ `--compact`) liệt kê tool của một app |

**Có cửa sổ hay không:**
- Mặc định, app chạy **headless** (không hiện cửa sổ, nhanh). Nếu cửa sổ app đang mở và có
  kênh điều khiển, gateway tự gắn vào cửa sổ đó.
- Gọi `artcraft_open` để mở cửa sổ và xem trực tiếp. Cửa sổ vẫn mở sau khi Claude hoặc Cursor đóng.
- Cửa sổ vừa mở thường chưa có tài liệu. Khi đó cần tạo mới trước, ví dụ `command_run` với id `file.new`.
- PdfCraft chỉ có chế độ headless.

**Cursor** được đăng ký với `--compact`, tức là chỉ hiện 4 tool `artcraft_*`, vì Cursor giới hạn số
tool gửi cho agent. Agent dùng `artcraft_tools(app)` để xem tool của app, rồi gọi `artcraft_call`.
Claude Code được đăng ký với đầy đủ tool.

## App được tìm ở đâu

Gateway tự tìm `<id>-cli.exe` trong `Program Files\<App>\`, tức là nơi ArtCraft Launcher cài app,
và trong `%LOCALAPPDATA%\Programs\<App>\`. Thư mục khác thì khai báo trong
`%LOCALAPPDATA%\ArtCraft\artcraft-mcp\config.json`:

```json
{
  "searchDirs": ["D:\\Apps"],
  "folder": "D:\\ArtCraftWork",
  "gui": "auto",
  "apps": {
    "vectorcraft": { "cli": "D:\\Apps\\VectorCraft\\vectorcraft-cli.exe", "port": 7979 },
    "filmcraft": { "disabled": true }
  }
}
```

- `folder`: giới hạn thư mục mà app headless được đọc và ghi. Hỗ trợ PhotoCraft, VectorCraft,
  WordCraft, SoundCraft (`--automation-*-root`) và PdfCraft (`--root`).
- `gui: "never"`: không bao giờ điều khiển cửa sổ. Tương đương tham số `--headless`.
- `apps.<id>`: đường dẫn CLI riêng, port riêng, tham số thêm (`args`), hoặc tắt app (`disabled`).

Kiểm tra nhanh: `artcraft-mcp.exe --list` liệt kê app tìm thấy và gọi thử MCP server của từng app.

Port điều khiển mà gateway dùng khi mở cửa sổ, mỗi app một port để mở cùng lúc được:
photocraft 7971, vectorcraft 7979, filmcraft 9876, lightcraft 7980, effectcraft 9877, designcraft 7984,
figcraft 7985, wordcraft 7981, soundcraft 7801, deckcraft 7990, cadcraft 7982, gridcraft 7983.

Dữ liệu nằm trong `%LOCALAPPDATA%\ArtCraft\artcraft-mcp\`:
- `tools-cache.json`: danh sách tool, làm mới khi exe của app thay đổi.
- `logs\<id>.log`: stderr của từng app.
- `photocraft.token`: token dùng chung giữa cửa sổ PhotoCraft và cầu nối.

## Phát triển

```powershell
cd tools\artcraft-mcp
cargo test
cargo build --release          # target\release\artcraft-mcp.exe
..\..\scripts\install-mcp.ps1 -Exe target\release\artcraft-mcp.exe   # cài bản vừa build
```

Mỗi lần push lên `main` có thay đổi trong `tools/artcraft-mcp/`,
`.github/workflows/artcraft-mcp.yml` sẽ test, build rồi phát hành `artcraft-mcp-v<version>` lên
Releases của repo này. Đổi `version` trong `Cargo.toml` khi muốn ra bản mới. Nếu push lại cùng
version, file của bản đó sẽ được thay.

Chỉ phụ thuộc `serde_json`. Gateway nói chuẩn MCP (`initialize`, `tools/list`, `tools/call`) với
các app, không dùng kênh JSON nội bộ của app. Nhờ vậy nó vẫn chạy khi đồng bộ upstream.
