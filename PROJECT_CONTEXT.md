# RocketLauncher Project Context

This file is a persistent memory snapshot for future chats.
Use it as a quick orientation before making changes.

## What This Project Is

`RocketLauncher` is a desktop launcher for Need for Speed: World private servers.
It uses:

- Frontend: Next.js (App Router, static export), React, Zustand, Tailwind
- Desktop runtime: Tauri 2
- Native backend: Rust (`src-tauri`)

The app runs as a single-shell UI with internal page switching via Zustand state.

## Top-Level Structure

- `src/`: frontend app code
  - `app/`: root (`layout.tsx`, `page.tsx`)
  - `components/`: UI, screens, forms
  - `stores/`: Zustand stores
  - `lib/`: APIs, types, config, utilities
- `src-tauri/`: Rust/Tauri backend
  - `src/lib.rs`: Tauri setup + command registration
  - `src/commands.rs`: launch/system/update/security commands
  - `src/downloader.rs`: game download/verify/repair/mods
  - `src/game_state.rs`: in-game state parsing
- `scripts/`: build/release wrappers
- `dist/`: generated release outputs

## Runtime Model

1. Frontend starts in Tauri webview.
2. UI calls Rust commands through `invoke` wrappers in `src/lib/tauri-api.ts`.
3. Rust emits events (`download-progress`, `verify-progress`, `game-running`, `game-exited`, `game-crashed`) consumed by frontend components.
4. Game launch/proxy/session handling are native-side responsibilities.

## Core Frontend Hotspots

- `src/app/page.tsx`: app shell and high-level orchestration
- `src/components/forms/LoginForm.tsx`: login + launch flow
- `src/components/screens/MainScreen.tsx`: server detail and download state
- `src/components/screens/SettingsScreen.tsx`: settings/security/verify controls
- `src/components/layout/ServerListPanel.tsx`: server fetching/selection
- `src/components/layout/TopBar.tsx`: global controls and update entry points

## Core Backend Hotspots

- `src-tauri/src/lib.rs`: command/event wiring
- `src-tauri/src/commands.rs`: large command surface (launch, update, system)
- `src-tauri/src/downloader.rs`: large downloader/mod/verification logic
- `src-tauri/src/game_state.rs`: game-state extraction for RPC

## Zustand Stores

- `src/stores/launcherStore.ts`: page, auth/session, game/download state
- `src/stores/serverStore.ts`: server list/order/selection
- `src/stores/settingsStore.ts`: launcher settings
- `src/stores/credentialsStore.ts`: stored credentials (persistent)
- `src/stores/updateStore.ts`: update UI state
- `src/stores/playtimeStore.ts`: tracked playtime

## Tauri Command Bridge

Primary TypeScript bridge:

- `src/lib/tauri-api.ts`

Most feature work touching native behavior should update both:

- Rust command implementation (`src-tauri/src/*.rs`)
- TS wrapper and caller usage (`src/lib/tauri-api.ts` + components)

## Build and Release

- Primary script: `scripts/tauri-wrapper.mjs`
- Next build output feeds Tauri bundle
- Version/channel must remain aligned across:
  - `package.json`
  - `src-tauri/Cargo.toml`
  - `src-tauri/tauri.conf.json`
  - build-time env values used by Rust update logic

## Known Risk / Debt Areas

- `commands.rs` and `downloader.rs` are large and hard to maintain.
- Some legacy/duplicate frontend modules exist and may be unused.
- Credentials are persisted locally in frontend store.
- Tauri security settings (CSP/capabilities) should be reviewed carefully before distribution.
- Build scripts overlap; prefer `scripts/tauri-wrapper.mjs` as source of truth.

## Recommended Workflow for New Chats

1. Read this file first.
2. Confirm target area (frontend UI, store logic, Rust command, build/update pipeline).
3. If changing command behavior, verify both invoke wrapper and event listeners.
4. Run targeted checks/tests after edits.

## Quick Prompt for New Chat

Use this starter prompt in a new chat:

> Read `PROJECT_CONTEXT.md` and use it as memory base. Then help me with: [your task].

