# Implementation Plan

## Upstream audit summary

`steipete/CodexBar` is split into two very different layers:

- `Sources/CodexBarCore`: reusable provider descriptors, fetch pipelines, parsers, and models
- `Sources/CodexBar`: macOS-only app shell using AppKit, SwiftUI, WidgetKit, Sparkle, and macOS cookie/keychain paths

This means a direct Linux port of the full desktop app is not the right starting point.

## Most important finding

The current upstream Linux path is CLI-first, not desktop-first.

That is useful for providers that already work through CLI, OAuth, API keys, or local files, but it does not solve OpenCode on Linux because upstream OpenCode support depends on macOS-only browser cookie import.

Relevant upstream files:

- `upstream-codexbar/Sources/CodexBarCore/Providers/OpenCode/OpenCodeProviderDescriptor.swift`
- `upstream-codexbar/Sources/CodexBarCore/Providers/OpenCode/OpenCodeUsageFetcher.swift`
- `upstream-codexbar/Sources/CodexBarCore/Providers/OpenCode/OpenCodeWebCookieSupport.swift`
- `upstream-codexbar/Sources/CodexBarCore/Providers/OpenCode/OpenCodeCookieImporter.swift`

## Recommended architecture

Use a hybrid Linux design instead of a literal port:

1. Tauri desktop shell for tray/window/settings behavior on Linux.
2. Rust host layer for polling, persistence, and desktop integration.
3. Provider adapter layer with two modes:
   - CLI-backed adapters for providers already usable on Linux
   - Linux-native web/cookie adapters for providers like OpenCode

## Why this is the safest path

- We avoid fighting AppKit/WebKit/Keychain macOS assumptions.
- We keep the UI Linux-native enough for Fedora KDE.
- We can start with your actual pain point instead of rebuilding upstream complexity first.
- We can still mirror upstream provider behavior and naming where helpful.

## Current status

- Remote repo created: `Yajush-afk/linux-codexbar`
- Fedora/KDE tray-first shell implemented
- Config file bootstrap implemented
- OpenCode manual-cookie provider implemented
- Codex OAuth-file provider implemented
- Claude OAuth-file provider implemented

## Proposed implementation phases

## Agreed v1 scope

- Providers: OpenCode, Codex, Claude
- UI mode: tray only
- Target environment: Fedora KDE Plasma first

## Fedora KDE notes

- This repo is currently targeting Fedora 43 KDE Plasma first.
- For Tauri desktop development on Fedora, the key packages are:
  - `webkit2gtk4.1-devel`
  - `openssl-devel`
  - `curl`
  - `wget`
  - `file`
  - `libappindicator-gtk3-devel`
  - `librsvg2-devel`
  - `libxdo-devel`
- Tauri Linux tray support has an important constraint: tray icon mouse events are not reliably emitted on Linux.
- Because of that, v1 should be menu-first instead of click-first:
  - the tray menu is the primary interaction surface
  - no critical flow should depend on left-click tray events
  - refresh, diagnostics, settings, and quit should all be available as tray menu actions

### Phase 1

Status: complete

- Build Fedora/KDE tray-capable shell
- Add app settings storage
- Define shared provider snapshot model
- Add tray menu sections for provider summaries, refresh state, and diagnostics
- Keep the app tray-only with no permanent dashboard window in v1

### Phase 2

Status: complete for manual-cookie path

- Implement OpenCode provider on Linux
- Support manual cookie header entry first
- Add optional Chromium/Firefox cookie import on Linux later in the phase
- Fetch workspace and subscription usage windows

### Phase 3

Status: complete for OAuth-file path

- Implement Codex provider for Linux
- Start with local auth-file/OAuth-driven usage and reset windows
- Decide separately whether OpenAI dashboard extras belong in v1 or later

### Phase 4

Status: complete for OAuth-file path

- Implement Claude provider for Linux
- Start with the most reliable Linux-compatible source path first
- Add diagnostics for auth source, polling failures, and reset windows

### Phase 5

Status: next

- Add packaging and autostart support for Fedora/KDE
- Improve tray compactness and notifications
- Add import/export for config and provider settings

## Next work

1. Add Fedora/KDE packaging and autostart support.
2. Add optional OpenCode browser-cookie import for Linux browsers.
3. Decide whether Codex dashboard extras belong in the next milestone.
4. Improve validation and diagnostics around expired credentials and malformed config.
