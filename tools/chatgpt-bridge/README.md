# ArtCraft ChatGPT

A small Electron app that PhotoCraft sends image requests to; chatgpt.com itself runs in your
own Chrome (or Edge), in a profile of its own, signed in once with your ChatGPT account.
Requests use your ChatGPT plan's usage; there is no API key, and nothing here reads or stores a
password (the sign-in lives in that Chrome profile,
`%LOCALAPPDATA%\ArtCraft ChatGPT\chrome-profile`).

Google refuses to sign in inside embedded browsers ("This browser or app may not be secure"),
which is why the page runs in real Chrome: the sign-in window is plain Chrome with no
automation; requests later start the same profile with a DevTools port on 127.0.0.1
(`chrome.js`), extensions off.

## Use

1. Install: `scripts\install-chatgpt-bridge.ps1` (needs Node.js). It builds the app and puts it in
   `%LOCALAPPDATA%\Programs\ArtCraft ChatGPT\`, with a Start menu shortcut.
2. Open **ArtCraft ChatGPT**, press **Đăng nhập bằng Chrome**, sign in to ChatGPT in the Chrome
   window that opens (Google, e-mail, Microsoft or Apple), then press **Kiểm tra**. Once only:
   later requests reuse the sign-in.
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
- A request runs in that Chrome tab as a person would: a new chat, the picture attached, the
  request typed and sent, then the image ChatGPT makes is downloaded and returned. One request at
  a time.
- The page is driven through its DOM (`page.js`: `COMPOSER`, `SEND`, `USER`, `ASSISTANT`,
  `STATE`). When chatgpt.com changes its page these are what to update.

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
