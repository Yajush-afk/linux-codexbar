# Linux CodexBar

Linux-first desktop companion for tracking AI coding tool usage windows, reset times, and limits.

Current state: Fedora KDE tray-first prototype with OpenCode, Codex, and Claude provider support.

## Why this repo exists

The upstream `steipete/CodexBar` project has a strong shared fetch/parsing architecture, but its desktop app is deeply macOS-specific. For your Fedora KDE workflow, the practical path is a Linux-native shell that can:

- reuse upstream ideas and data models where they make sense
- support Linux tray behavior cleanly
- add Linux-specific cookie/session integrations for providers like OpenCode

## Initial stack

- Tauri v2
- React 19 + TypeScript
- Rust host layer for desktop integration and background orchestration

## Current scope

- Target desktop: Fedora 43 KDE Plasma first
- UI: tray menu only
- Providers:
  - OpenCode via manual cookie header in `config.json`
  - Codex via local `~/.codex/auth.json`
  - Claude via local `~/.claude/.credentials.json`

## Fedora prerequisites

Install the Tauri Linux dependencies first:

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install "c-development"
```

## Run the app

```bash
npm install
npm run tauri dev
```

What you should expect:

- this is not a normal localhost-style web demo
- Vite still runs locally during development, but the real demo is the desktop tray app
- after `npm run tauri dev`, look in the KDE system tray for `Linux CodexBar`
- open the tray menu to view provider status lines and actions

## How to check it during development

1. Run `npm run tauri dev`.
2. Open the tray icon/menu in KDE Plasma.
3. Click `Open config.json` from the tray.
4. Add your OpenCode cookie header if needed.
5. Use `Reload config`.
6. Use `Refresh now`.
7. Verify that the provider lines in the tray menu update.

During dev, Tauri may also start a frontend dev server on a localhost port, but that is backing the app runtime. The main thing you verify is the tray behavior, not a browser page.

## Config file

The app creates `~/.config/linux-codexbar/config.json` on first run.

OpenCode currently expects a manual cookie header in that file. Codex and Claude read their existing local auth files directly.

## Notes

- Linux tray behavior is menu-first on purpose.
- OpenCode browser-cookie import is not implemented yet; manual cookie entry is the first auth path.
- The upstream repo clone used for architecture research lives at `../upstream-codexbar`.
