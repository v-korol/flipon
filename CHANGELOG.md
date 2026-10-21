# Changelog

Release notes for FlipOn. `pnpm release` reads the section for the version it publishes and
shows it to users in the update notice.

## Unreleased

- macOS: FlipOn now needs macOS 13 Ventura or later (its interface needs a newer Safari engine
  than macOS 11 and 12 ship with). Logins are double-checked after they are saved to the Keychain,
  and `claude` is found even when it was installed with nvm, fnm, asdf, mise or pnpm.

## 0.2.1

- Automatic updates are now on by default, so new versions install by themselves when FlipOn is
  idle. You can turn this off in Settings → About; your choice is kept.
- Settings → About has a "Send feedback" button for bugs, ideas and questions.
- Usage counting is more accurate: one copy counts once a day, whatever network you are on. It still
  sends nothing that identifies you or your computer.

## 0.2.0

- FlipOn now runs on macOS 11 or later, Apple Silicon and Intel. On a Mac it reads and writes Claude
  Code's login in the Keychain, the same place the CLI keeps it, and the icon follows the light or
  dark menu bar. The app is not notarized by Apple: the first launch asks you to press "Open Anyway"
  in System Settings → Privacy & Security.
- Finding `claude` works better when FlipOn is started from Finder or at login (Homebrew, nvm, volta
  and the native installer folders are searched).

## 0.1.1

- FlipOn now updates itself. New versions come from korol.app, are checked against our signature
  before they are installed, and install when you click "Restart to update" (or by themselves if
  "Install updates automatically" is on).
- The tray menu and Settings → About show the update status, with a "Check now" button.

## 0.1.0

- First release: keep several Claude Code logins, switch between them from the tray, live usage
  for every account, four automatic switching strategies, and a customisable tray icon.
