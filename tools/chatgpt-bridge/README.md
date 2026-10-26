# ArtCraft ChatGPT

chatgpt.com in its own window (Electron), signed in once with your own ChatGPT account, that
PhotoCraft sends image requests to. Requests use your ChatGPT plan's usage; there is no API key,
and nothing here reads or stores a password (the sign-in lives in the window's own browser
profile, partition `persist:chatgpt`).

## Use

1. Install: `scripts\install-chatgpt-bridge.ps1` (needs Node.js). It builds the app and puts it in
   `%LOCALAPPDATA%\Programs\ArtCraft ChatGPT\`, with a Start menu shortcut.
2. Open **ArtCraft ChatGPT** once and log in to ChatGPT in its window. If Google sign-in refuses
   the window, use e-mail, Microsoft or Apple sign-in.
3. In PhotoCraft: **Window › AI / MCP Command Center › AI Chat**, Agent **ChatGPT (ArtCraft
   ChatGPT)**, type the request (e.g. `Tách nền ảnh này`, `Đổi bầu trời thành hoàng hôn`), Send.
   PhotoCraft sends the flattened image, ChatGPT edits it, and the result lands as a new layer
   named "ChatGPT" (Undo changes / Keep changes as for the other agents). PhotoCraft starts the
   window by itself when it isn't running.

Commands PhotoCraft knows by name (e.g. `Hoàn tác`, `Gaussian Blur`) still run in the app without
AI, as with every agent.

## How it works

- The app listens on `127.0.0.1` on a random port with a random token, written to
  `%APPDATA%\ArtCraft\chatgpt-bridge.json` (`{port, token, pid, version}`) while it runs.
- `GET /v1/status` → `{ok, loggedIn, busy, version}`;
  `POST /v1/image {prompt, image?: base64 PNG}` → `{ok, image, mime, text}` or
  `{ok: false, code: login|limit|noimage|timeout|…, error}`. Both need `Authorization: Bearer <token>`.
- A request runs in the window as a person would: a new chat, the picture attached, the request
  typed and sent, then the image ChatGPT makes is downloaded and returned. One request at a time.
- The page is driven through its DOM (`main.js`: `COMPOSER`, `SEND`, `USER`, `STATE`). When
  chatgpt.com changes its page these selectors are what to update.

Automating the ChatGPT website is not an official OpenAI interface: it can break when the site
changes, and OpenAI's terms limit automated use of ChatGPT. It runs one request at a time, in
your own signed-in window, only when you send one.

## Develop

```powershell
cd tools\chatgpt-bridge
npm ci
npm test                      # lib.js
$env:ELECTRON_RUN_AS_NODE=$null; npx electron .   # run from source
```
