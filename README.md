<div align="center">

<img src="docs/logo.svg" width="96" alt="FlipOn logo" />

# FlipOn

Keep every Claude login in your tray. Switch with one click, or let FlipOn move you to the next account before you hit a limit.

Free. No account. Source available. Not affiliated with Anthropic.

</div>

---

## What it does

Claude subscriptions are rate limited on a rolling 5 hour window and a rolling 7 day
window. If you have more than one account (work, personal, a spare), hitting the limit
means logging out of the Claude Code CLI, logging in to another account in the
browser, and remembering to switch back later.

FlipOn keeps the login of every account you add and changes which one the CLI uses:

* **One click, no browser.** Switching swaps the saved login in place. Nothing is
  signed out, and CLI sessions that are already running are not interrupted.
* **Live usage for every account.** The 5 hour, weekly and per model windows, how fast
  they are moving, and when each one resets. Shown in the tray icon and in a small
  popover.
* **Automatic switching.** Turn it on and FlipOn moves you to another account before
  the current one runs out, using the strategy you pick.
* **A heads up before the limit.** An optional notification at a usage level you
  choose, even if you switch by hand.

## Switching strategies

| Strategy | What it does |
| --- | --- |
| **Most headroom** | Stays on the current account until it reaches the switch limit, then moves to the one with the most usage left. |
| **My account order** | Always uses the top account in your list that still has room. Moves down when it fills up and jumps back as soon as it resets. |
| **Balanced usage** | Uses first the account whose weekly limit resets soonest, so leftover quota is not lost at the reset. If tied, the account with a session already running wins. |
| **Round-robin by threshold** | Uses each account up to a set percent, then moves to the next in your list and loops around, with a higher cap on every lap. |

Pin an account for 1 to 4 hours, or until you switch again, from the Switch button. A
pinned account is only left when it hits its own limit.

## Tray icon

Pick a look in Settings: progress bar, percentage, ring, traffic light dot, mood face
or a tiny usage history. Or build your own from the account name, the 5 hour bar, the
weekly bar, today's cost and a reset countdown.

## Install

Download **[FlipOn-Setup.exe](https://flipon.korol.app/download)** from
[flipon.korol.app](https://flipon.korol.app) and run it. It is always the newest version.
FlipOn is built and tested on **Windows 10 and 11**. The page links the VirusTotal report of the current file.

The installer is not code signed yet, so Windows SmartScreen may warn you: choose
**More info**, then **Run anyway**. For the same reason a few antivirus engines on
VirusTotal can flag an unsigned new file as suspicious (machine learning guesses, not a
named threat). The code is public, and you can build the files yourself, see
[DEVELOPMENT.md](DEVELOPMENT.md).

You need the [Claude Code CLI](https://docs.claude.com/en/docs/claude-code) installed
and logged in. FlipOn finds `claude` on its own; if it cannot, set the path in
Settings → System.

**macOS** (13 Ventura or later, Apple Silicon and Intel) is supported from 0.2.0. FlipOn reads and writes
Claude Code's login in the Keychain, where the CLI keeps it. The app is not notarized by Apple (that
needs a paid developer account), so the first launch needs one extra step: open the `.dmg`, drag
FlipOn to Applications, then System Settings → Privacy & Security → **Open Anyway**. Linux is
untested.

## Getting started

1. Start FlipOn. Its icon appears in the tray (on Windows 11 it may sit behind the ^
   arrow: drag it onto the taskbar).
2. If the CLI is already logged in, the popover offers **Save it**. Otherwise click
   **Add** and sign in through your browser.
3. Add your other accounts the same way, then pick a strategy in Settings → Switching.

## How it works, and what it touches

Claude Code keeps its login in `~/.claude/.credentials.json` and the account details in
`~/.claude.json`. To switch, FlipOn replaces only the login (`claudeAiOauth`) and the
account details (`oauthAccount`) with the saved ones. Everything else in those files,
such as MCP logins and your settings, is left alone. Before it switches it reads the
CLI's files first, so tokens the CLI refreshed on its own are not lost.

Usage comes from Anthropic's OAuth usage endpoint. It is not a documented API, so it
can change without notice. Cost figures ("today") are an estimate from your local
Claude Code session files at public API prices; a subscription is not billed per token.

Do not run another account switcher at the same time: two tools swapping the same
files and refreshing the same tokens will invalidate each other's logins.

## Updates

FlipOn updates itself from its own server, `api.korol.app`. It checks every few hours,
downloads a new version in the background and only installs builds signed with the
project's key. It installs by itself when FlipOn is idle (turn this off in Settings → About), or
click **Restart to update** when you see "Version X is ready" in the popover or the tray.

## Privacy

Everything stays on your computer. There is no account and no server of ours.

* Saved logins are kept in FlipOn's data folder (`%APPDATA%\FlipOn` on Windows) with
  access limited to your user. They are **not encrypted** beyond that.
* The internet is used for two things. Talking to Anthropic: reading usage, and
  refreshing a login that is about to expire. And checking for updates at `api.korol.app`
  every few hours, which sends the app version, the platform and a token that changes every
  day (a hash of a random number made on your computer and the date; the number itself never
  leaves it). The server uses it only to count how many copies check in each day: tomorrow's
  token cannot be linked to today's, and no address or ID is stored. Credentials are never sent anywhere except Anthropic.
* Removing an account only forgets FlipOn's copy. It does not sign the CLI out.

## Known limits

* macOS support is new and was written without a Mac to test on; if something misbehaves, please
  open an issue. Linux builds may work but are untested.
* The installer is not code signed yet (see Install).
* "Log in again" and automatic switching depend on how the CLI behaves; if Anthropic
  changes the login or usage format, FlipOn may need an update.

## Source and license

FlipOn is free, and its source code is public so you can read exactly what it does.
It is source available under the [Functional Source License](LICENSE.md)
(FSL-1.1-MIT): you may use, study and change it for any purpose except offering a
competing product, and every version becomes plain MIT two years after its release.

FlipOn is an independent project and is not affiliated with, endorsed by or sponsored
by Anthropic. "Claude" is a trademark of Anthropic.

## For developers

Build, test and project layout are in [DEVELOPMENT.md](DEVELOPMENT.md).

## Disclaimer: use at your own risk

FlipOn is an unofficial tool. It is not made, reviewed or approved by Anthropic.

Anthropic's [terms for Claude Code](https://code.claude.com/docs/en/legal-and-compliance)
say that OAuth sign-in is meant for ordinary use of Claude Code and Anthropic's own
apps, that third-party developers may not route requests through subscription
credentials on behalf of users, and that developers may not collect, store or
intermediate Claude.ai credentials. They also say Anthropic may enforce this without
prior notice, and that plan limits assume ordinary, individual use.

FlipOn keeps copies of your own logins on your own computer and switches between
them. It signs you in only through the official `claude auth login`, and it sends
your credentials only to Anthropic. Whether this fits Anthropic's terms, now or in the
future, is not something the author can promise. Anthropic may change its rules or
its systems at any time.

By using FlipOn you accept that:

* **You are fully responsible for how you use it**, and for your Claude accounts.
* **The author is not responsible** if any Claude account is limited, restricted,
  suspended or banned, or for any other loss or damage, including lost access, data or
  money. The software comes with no warranty (see [LICENSE.md](LICENSE.md)).
* It is your job to read Anthropic's current terms and decide whether to use FlipOn.

If you are not comfortable with that, do not use it.
