# MCP, CLI và tự động hóa

Kiểm tra trực tiếp trong source ngày 2026-10-14 (file `apps/<id>/apps/<id>-cli/src/main.rs`,
`apps/<id>/apps/<id>/src/control_server.rs`, crate `mcp`/`automation`, README và `docs/mcp.md`
của từng app). **Chưa chạy thử** vì máy chưa cài Rust. Xác nhận sau khi build bằng
`builds\<id>\<ver>\windows-x64\<id>-cli.exe mcp`.

## Kết quả: cả 13 app đều có MCP server và CLI trong source

Mô hình chung của bộ Craft:

```
               ┌────────────── <id>-cli.exe ───────────────┐
Claude Code ──stdio──► mcp  (JSON-RPC 2.0, Model Context Protocol)
               │        ├─ headless: engine chạy trong tiến trình CLI, không mở cửa sổ
               │        └─ bridge: --connect/--bridge/--attach PORT
               └───────────────────────┬───────────────────┘
                                       │ TCP 127.0.0.1:PORT, JSON lines
                                       ▼
                       <id>.exe --control PORT   (app GUI đang mở)
```

Mọi menu, công cụ, nút đều là một "command" trong registry của engine. UI, CLI, kênh điều khiển
và MCP gọi cùng một registry. `<id>-cli commands` liệt kê toàn bộ command.

| App | MCP headless | Nối vào app đang mở | Mở kênh điều khiển ở GUI | Ghi chú |
|-----|--------------|---------------------|--------------------------|---------|
| PhotoCraft | `photocraft-cli mcp` | `--bridge 127.0.0.1:PORT` (+ `--control-token`) | `photocraft --control PORT` | Có token bảo vệ. `--automation-read-root/--automation-write-root` giới hạn thư mục. Còn có `serve` (JSON lines) |
| VectorCraft | `vectorcraft-cli mcp` | `--connect 127.0.0.1:7979` | `vectorcraft --control 7979` | Giới hạn thư mục bằng `--automation-*-root` |
| FilmCraft | `filmcraft-cli mcp --demo` | `--bridge 127.0.0.1:9876` | `filmcraft --control 9876` | Upstream có sẵn `.mcp.json`. Tài liệu: `docs/agents.md`, `docs/control-protocol.md` |
| LightCraft | `lightcraft-cli mcp [FILES/FOLDERS]` | `--connect 127.0.0.1:7980` | `lightcraft --control 7980` | Có screenshot, click, gesture khi bridge. `docs/mcp.md` |
| PdfCraft | `pdfcraft-cli mcp [--root DIR] [--compact]` | không có bridge MCP. GUI điều khiển qua `pdfcraft-cli ui --control FILE <method>` | `pdfcraft --control FILE` | MCP chỉ chạy khi agent gọi, không mở cổng mạng. Feature `mcp` bật mặc định |
| EffectCraft | `effectcraft-cli mcp [--autosave]` | `--bridge 9877` | `effectcraft --control 9877` | Upstream có sẵn `.mcp.json` |
| DesignCraft | `designcraft-cli mcp [--sample]` | `--connect 7979` | `designcraft --control 7979` | `docs/mcp.md` |
| FigCraft | `figcraft-cli mcp` | `--attach PORT` | `figcraft --control PORT` | Có thêm `figcraft-server` (máy chủ plugin/cộng tác) |
| WordCraft | `wordcraft-cli mcp` | `--connect 127.0.0.1:PORT` | `wordcraft --control 7981` | Mỗi request cần key của cửa sổ (file key). `--author` |
| SoundCraft | `soundcraft-cli mcp [--demo]` | `--connect 7801` | `soundcraft --control 7801` | Headless không dùng thiết bị âm thanh |
| DeckCraft | `deckcraft-cli mcp [--sample]` | `--connect 7990` | `deckcraft --control 7990` | |
| CADCraft | `cadcraft-cli mcp` | `--connect HOST:PORT [--lazy] [--launch]` | `cadcraft --control 7979` | `--launch` tự mở app |
| GridCraft | `gridcraft-cli mcp [--in book.xlsx \| --sample budget]` | `--connect 7979` | `gridcraft --control 7979` | |

Port trong bảng là ví dụ trong README của từng app. Nhiều app dùng chung 7979 nên khi mở nhiều
app cùng lúc, hãy chọn port khác nhau. Mọi kênh điều khiển chỉ bind `127.0.0.1` (không lộ ra mạng).

**CLI không qua MCP** (dùng trong script/CI), ví dụ:
`<id>-cli info FILE`, `<id>-cli convert IN OUT`, `<id>-cli run SCRIPT`, `<id>-cli commands`.
Tham số cụ thể: `<id>-cli --help`.

### Chưa hỗ trợ / giới hạn (theo source)

- PdfCraft không có chế độ MCP "bridge" vào app GUI (chỉ headless). GUI dùng `pdfcraft-cli ui`.
- Không app nào có HTTP/REST API công khai. Kênh điều khiển là JSON lines qua TCP loopback, dành
  cho CLI/MCP của chính app đó.
- Các app không gọi được nhau (ví dụ chưa có thao tác "gửi ảnh từ PhotoCraft sang DesignCraft").
  Muốn phối hợp thì agent phải đi qua file trung gian.
- Bản web (`<id>-web`) không có MCP.

## Kết nối với Claude Code / Cursor

Sau khi build (đường dẫn ví dụ cho PdfCraft 0.8.0):

```powershell
claude mcp add pdfcraft -- "D:\CLAUDE CURSO WEBSITE LUAN\APP ADOBE  ARTCRAFT LUAN DESIGNER\LUAN-ARTCRAFT-SUITE\builds\pdfcraft\0.8.0\windows-x64\pdfcraft-cli.exe" mcp --root "D:\PDF"
```

Hoặc dùng file cấu hình mẫu `config/mcp.example.json` (đổi đường dẫn và phiên bản rồi chép vào
`.mcp.json` ở gốc repo cho Claude Code, hoặc `.cursor/mcp.json` cho Cursor). File mẫu **không** được
bật sẵn vì các `.exe` chưa được build.

Khuyến nghị an toàn: luôn đặt `--automation-read-root` / `--automation-write-root` (hoặc `--root`
với PdfCraft) để giới hạn thư mục agent được đọc/ghi.

## MCP tổng: ArtCraft MCP (`tools/artcraft-mcp`, ĐÃ triển khai 15/10/2026)

Một MCP server duy nhất cho Claude Code và Cursor, điều khiển mọi app ArtCraft đã cài. Hướng dẫn
dùng, link tải và cấu hình nằm trong [tools/artcraft-mcp/README.md](../tools/artcraft-mcp/README.md).

```
Claude Code / Cursor
        │ stdio (MCP)
        ▼
┌──────────────────────── artcraft-mcp.exe ─────────────────────────┐
│ Tìm app: Program Files\<App>\<id>-cli.exe (+ searchDirs config)   │
│ Lazy spawn: `<id>-cli mcp ...` chỉ khởi động khi cần              │
│ Tên tool: draw_shape của vectorcraft -> vectorcraft_draw_shape   │
│ tools/list = hợp nhất (cache theo exe, làm mới khi app cập nhật)  │
│ Meta tools: artcraft_apps, artcraft_open, artcraft_call           │
└───────┬──────────────┬──────────────┬─────────────────────────────┘
        │stdio         │stdio         │stdio      (một tiến trình con mỗi app)
   photocraft-cli   vectorcraft-cli  gridcraft-cli ...
        │ (--bridge / --connect / --attach khi cửa sổ đang mở)
   photocraft.exe --control 7971
```

So với bản đề xuất cũ:
- Tên tool dùng `_` thay cho `.`, vì MCP chỉ cho phép `[A-Za-z0-9_-]`.
- Gateway tự viết JSON-RPC và chỉ cần `serde_json`, không dùng `rmcp`.
- Port do gateway cấp, mỗi app một port. VectorCraft giữ 7979.
- Tool của mọi app đã cài được bật sẵn: 4 app trên máy này có tổng 94 tool. Cursor dùng `--compact`.

Đã kiểm tra thật trên máy này (15/10/2026):
- Tìm thấy PhotoCraft, WordCraft và GridCraft trong `Program Files`, VectorCraft trong `D:\Apps`.
  MCP của cả 4 app đều trả lời: 25, 16, 21 và 32 tool.
- Gọi headless `gridcraft_list_commands` và `vectorcraft doc_inspect` thành công.
- `artcraft_open vectorcraft` mở cửa sổ và gắn vào trong 1,1 giây. Sau đó `file.new`, `draw_shape`
  (ngôi sao) và `add_text` hiện ngay trên cửa sổ.
- `artcraft_open gridcraft` mở cửa sổ. Cửa sổ vẫn còn sau khi phiên MCP đóng, và phiên kết thúc
  không bị treo.

Chưa kiểm tra: FilmCraft, PdfCraft, EffectCraft và CADCraft (chưa cài trên máy này). Cửa sổ
PhotoCraft qua token file cũng chưa thử.
