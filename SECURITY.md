# Security Policy

Stint records what you do on your computer, so it has to be trustworthy. This document describes what it collects, how that data is protected, and how to report problems.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Use GitHub's **private vulnerability reporting** (Security tab → "Report a vulnerability"). We aim to acknowledge reports within 72 hours and to publish a fix and advisory once one is available.

## What Stint collects

| Data | Collected? |
|---|---|
| Name of the foreground app | Yes |
| Start/end time of each period of use | Yes |
| Window titles | **Only if you turn on "Record window titles"** (off by default) |
| Keystrokes, clipboard, screenshots, screen contents, URLs, file contents | **Never** |
| Anything sent over the network | **Never.** Stint makes no network requests and has no telemetry or update pings. |

Idle time is measured only as "seconds since last input", a single number the OS provides. Stint does not observe which keys are pressed.

## Threat model

**In scope**
- A malicious web page or document sets its window title to an injection payload (HTML/JS, SQL, or a spreadsheet formula), hoping it gets executed when Stint displays, stores, or exports it.
- A compromised or buggy UI layer tries to make the Rust core do more than it should (read arbitrary files, run commands, reach the network).
- Other local user accounts trying to read your activity history.
- Supply-chain risk from dependencies and CI.

**Out of scope**
- Malware already running as *your* user account. It can read anything you can, including Stint's database. Use full-disk encryption (FileVault / BitLocker) to protect data at rest.

## Hardening measures

**UI / webview**
- Strict Content Security Policy: scripts, styles, and images from the app bundle only. No inline scripts, no `eval`, no remote origins. `connect-src` allows only Tauri's IPC.
- All rendering goes through `textContent` and DOM APIs. `innerHTML` and other HTML sinks are banned, and a CI check enforces this. Window titles are always shown as text.
- No frontend framework or third-party JavaScript, so there's no npm runtime supply chain.
- `freezePrototype` is enabled to blunt prototype-pollution gadgets.
- Devtools are disabled in release builds (Tauri default).

**IPC boundary**
- Tauri v2 capabilities: the window may call **only** Stint's 15 declared commands (`src-tauri/capabilities/main.json`). No filesystem, shell, HTTP, dialog, or other plugin APIs are exposed to JavaScript.
- Every command validates its arguments (ranges, enum values, string lengths, control characters) and treats the UI as untrusted.
- Export writes only to a fixed, app-chosen path in your Downloads folder, using `create_new` (never overwrites).

**Storage**
- SQLite via `rusqlite` with **bound parameters only**. No SQL is built from strings. A unit test stores SQL-injection payloads verbatim.
- On macOS/Linux, the data directory is `0700` and the database file is `0600` (owner only). On Windows it lives in your per-user `%APPDATA%`, which other standard users can't read.
- `PRAGMA secure_delete=ON` zeroes deleted content, and "Delete everything" / "Erase titles" also `VACUUM` the file, so removed data doesn't linger in free pages.
- `PRAGMA trusted_schema=OFF`.
- Optional automatic retention deletes old history.

**Export**
- CSV cells beginning with `=`, `+`, `-`, `@` or a tab are prefixed with `'` to prevent formula injection when opened in Excel or Sheets.

**Process**
- Single-instance lock, so two trackers never write to the same database.
- A panic while sampling is caught, and locks recover from poisoning. The tracker can't silently die mid-day.

**Supply chain / CI**
- `Cargo.lock` and `package-lock.json` are committed. CI builds with `--locked`.
- GitHub Actions are pinned to full commit SHAs, use least-privilege `permissions`, and check out without persisting credentials.
- `cargo audit` (RustSec) and `npm audit` run in CI. Dependabot keeps dependencies and actions current.
- Release builds are created as **drafts** for human review. Code-signing hooks for macOS (Developer ID + notarization) are in `release.yml`. Signing releases before wide distribution is strongly recommended.

## Verifying a build

Release binaries are built by GitHub Actions from the tagged commit. To verify independently, check out the tag and run `npm ci && npm run build`.
