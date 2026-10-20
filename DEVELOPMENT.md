# Developing FlipOn

FlipOn is a Tauri 2 app: a Rust core (accounts, switching, usage polling, the tray
icon) and one React + TypeScript front end that renders two windows, the popover and
Settings. UI components follow [shadcn/ui](https://ui.shadcn.com) on Radix UI, with
Font Awesome icons.

## Commands

Needs Node 22+, pnpm, Rust (stable), and on Windows the Visual Studio C++ build tools
with WebView2.

| What | Command |
| --- | --- |
| Install | `pnpm install` |
| Run in development | `pnpm tauri dev` (front end on port 1520) |
| Rust tests | `cd src-tauri && cargo test` |
| Rust lint | `cd src-tauri && cargo clippy --all-targets` |
| Type check the front end | `pnpm exec tsc --noEmit` |
| Release installer | `pnpm tauri build` |

UI work without Rust: `pnpm dev`, then open `http://localhost:1520/?w=popover` or
`?w=settings`. Outside Tauri a mock backend (`src/lib/mock.ts`) supplies the data.

Regenerate every icon from `app-icon.png` (1024 px) with `pnpm tauri icon app-icon.png`.
The logo source is `docs/logo.svg`. To see every tray look as PNGs (dark and light taskbar):
`FLIPON_DUMP_DIR=some/folder cargo test dump_tray_icons`.

## Where things are

| File | What it does |
| --- | --- |
| `src-tauri/src/engine.rs` | The brain: account list, which account is active, usage polling, switching, the login flow, notifications. Everything the UI shows is a `Snapshot` built here. |
| `src-tauri/src/strategy.rs` | The four switching strategies as pure functions, with unit tests. |
| `src-tauri/src/claude_creds.rs` | Reads and writes the CLI's own login files. Replaces only `claudeAiOauth` and `oauthAccount`. |
| `src-tauri/src/store.rs` | FlipOn's own files: settings, state and one saved login per account. |
| `src-tauri/src/api.rs` | The usage endpoint and OAuth token refresh. Parses both response shapes. |
| `src-tauri/src/cli.rs` | Finds `claude` and runs `claude auth login`, forwarding the pasted code. |
| `src-tauri/src/activity.rs` | "Today" cost, turns and lines, from the local session transcripts. |
| `src-tauri/src/tray.rs`, `tray_icon.rs` | The tray menu, and the icon drawn with tiny-skia. |
| `src-tauri/src/windows.rs` | The popover (anchored to the tray icon) and the Settings window. |
| `src-tauri/src/commands.rs` | Everything the front end can call. |
| `src/components/popover/` | The popover: account cards, notices, dialogs. |
| `src/components/settings/` | The Settings window. |
| `src/components/ui/` | The shadcn style base components. |

FlipOn's data lives in `%APPDATA%\FlipOn\`: `settings.json`, `state.json` and
`credentials\<id>.json`.

## Things to know

* Windows: creating a window inside a synchronous command or a menu handler
  deadlocks the main thread. Keep `open_settings` async.
* The popover is a transparent, frameless window. It hides itself when it loses focus,
  and a tray click within 350 ms of that is ignored so it does not reopen at once.
* The usage endpoint is undocumented. `api::parse_usage` accepts the flat keys
  (`five_hour`, `seven_day`) and the newer `limits` array.
* Tests never touch your real login: the one that writes credentials points
  `CLAUDE_CONFIG_DIR` at a temporary folder.

## Debugging the real window

Set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` before
`pnpm tauri dev`, then attach to `http://127.0.0.1:9222/json` with any DevTools client.
Do not leave that port open.

## Releasing a macOS build

There is no Mac here, so macOS is built by Codemagic (`codemagic.yaml`) from the same release tag:

1. `pnpm release …` on Windows publishes the Windows build, commits and tags `v<version>`.
2. `git push origin main --tags`: Codemagic sees the tag, builds one universal app (Intel + Apple
   Silicon), runs the tests and runs `scripts/release-macos.mjs`, which uploads `FlipOn.dmg` (what
   people download) and `FlipOn.app.tar.gz` + its signature (what the auto-updater installs) to
   api.korol.app for `darwin-aarch64` and `darwin-x86_64`.
3. The site shows "Download for macOS" to Mac visitors as soon as the server has a macOS build.

One-time Codemagic setup: add the GitLab repo as an application, then create an environment variable
group `flipon_release` with secure variables `FLIPON_ADMIN_SECRET`, `TAURI_SIGNING_PRIVATE_KEY` (the
*contents* of `~/.tauri/flipon.key`) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty if none).
The free plan includes 500 macOS minutes a month; a build takes about 15.

The app is ad-hoc signed (`signingIdentity: "-"`) and not notarized, which is why users press
"Open Anyway" once. On a Mac the Claude Code login lives in the Keychain: service
`Claude Code-credentials` (plus `-<8 hex of sha256(config dir)>` with a custom `CLAUDE_CONFIG_DIR`),
account `claude-code-user`; `claude_creds.rs` mirrors how the CLI names and writes it. That part
could not be run on a real Mac while it was written: test it first on a Mac.

## Releasing

Releases are published to our own server (`api.korol.app`, the `@korol/backend` app in the
`st sites` repo), never to a third-party host, and installed copies update from there.

```
pnpm release patch          # or minor, major, or an exact 1.2.3
```

1. Write a `## <version>` section in `CHANGELOG.md` first: it becomes the update notes.
2. The script bumps the version (package.json, Cargo.toml, tauri.conf.json), builds the
   installer signed with the updater key, uploads it with its signature and notes, checks the
   server stored the same bytes, then commits and tags `v<version>`.
3. Installed copies find it within a few hours (or at once with "Check now").

Needs `FLIPON_ADMIN_SECRET` (the backend's `ADMIN_SECRET`) in the environment or in an
untracked `.env.release` file. Options: `--notes "…"`, `--no-build`, `--no-upload`, `--no-git`.
Copies of each release land in `release-artifacts/<version>/` (with `SHA256SUMS.txt`).

**The updater key** is `~/.tauri/flipon.key` (public half in `tauri.conf.json`). Back it up
somewhere safe and never commit it: without it no update can be signed, and installed copies
would have to be reinstalled by hand with a new key.

Pull a broken release with
`curl -X DELETE -H "Authorization: Bearer $FLIPON_ADMIN_SECRET" https://api.korol.app/apps/flipon/releases/<version>/windows-x86_64`
— installed copies then stay on (or are offered) the newest remaining version.

The installer is not code signed, so Windows SmartScreen warns on first download.
