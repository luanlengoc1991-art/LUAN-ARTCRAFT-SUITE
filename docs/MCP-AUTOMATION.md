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

## Đề xuất: MCP Gateway chung (CHƯA triển khai)

Mục tiêu: một MCP server duy nhất trong Cursor/Claude Code điều khiển được cả 13 app.

```
Claude Code / Cursor
        │ stdio (MCP)
        ▼
┌──────────────────────── artcraft-gateway ────────────────────────┐
│ Registry: đọc config/apps.json → 13 app, đường dẫn <id>-cli.exe  │
│ Lazy spawn: chỉ khởi động `<id>-cli mcp ...` khi có tool của app │
│ Namespacing: tool "export" của pdfcraft → "pdfcraft.export"      │
│ tools/list = hợp nhất tools/list của các app đã bật              │
│ Policy: allowlist app, read/write roots chung, timeout, log      │
│ Meta tools: suite.list_apps, suite.launch_app(id, port),         │
│             suite.status                                          │
└───────┬──────────────┬──────────────┬─────────────────────────────┘
        │stdio         │stdio         │stdio      (một tiến trình con mỗi app)
   photocraft-cli   pdfcraft-cli   gridcraft-cli ...
        │ (tùy chọn --connect / --bridge)
   photocraft.exe --control PORT
```

Thiết kế:

1. **Không sửa app nào.** Gateway chỉ là client MCP của 13 server có sẵn, nên khi đồng bộ
   upstream không bị xung đột.
2. **Viết bằng Rust**, đặt ở `tools/artcraft-gateway/` (workspace riêng, không đụng `apps/`). Dùng
   crate `rmcp`, vì FilmCraft và PhotoCraft đã dùng nó.
3. **Lazy + giới hạn:** chỉ spawn app khi cần, tắt khi idle. `tools/list` có thể rất lớn (hàng
   trăm command mỗi app), nên mặc định chỉ bật các app được cấu hình. Tool `suite.enable_app`
   bật thêm khi cần.
4. **Port tập trung:** gateway cấp port riêng cho mỗi app (7801–7813) để tránh trùng 7979.
5. **Bảo mật:** mọi app chạy với read/write roots do gateway đặt. Token/key file (PhotoCraft,
   WordCraft) do gateway quản lý.
6. **Rủi ro:** giao thức điều khiển của từng app đang thay đổi nhanh (alpha). Gateway chỉ dựa vào
   giao diện MCP chuẩn (`initialize`, `tools/list`, `tools/call`), không dựa vào kênh JSON nội bộ.

Bước tiếp theo nếu bạn đồng ý: build 2–3 app → chạy thử từng MCP server riêng → viết gateway
tối thiểu (registry + proxy tools/list và tools/call).
