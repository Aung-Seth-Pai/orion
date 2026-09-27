# Orion 🚀

**A Context-Switching & Focus Workspace for Windows.**

Orion is a zero-friction, single-context workspace launcher that instantly loads your project’s essential resources, notes, and focus timer—eliminating tab clutter and keeping you locked into one task at a time.

It lives in your system tray. Press `Ctrl + Shift + O` (rebindable in Settings), type a project name, and you are in: the right folders, the right docs, the right scripts, and a running timer. Nothing else competing for your attention.

## ✨ Key Features

* **Global Quick Search:** Press `Ctrl + Shift + O` from anywhere in Windows — or rebind it to any combination you prefer — to search and launch instantly. Narrow the results with slash command scoping: `/ws` for workspaces, `/link` for links, `/folder` for folders, `/script` for automation scripts, and `/ai` to search by meaning rather than by exact words.
* **Multi-Language Automation:** Write and execute PowerShell, CMD, Node.js, Python, and WSL Bash scripts directly from the UI, with per-workspace interpreter overrides.
* **Focus Analytics:** Independent Pomodoro and stopwatch timers per workspace that keep running while you work elsewhere in the app, native OS notification banners on completion, and resettable **Total Time** tracking so you can measure a sprint without losing your history.
* **Environment Variable Injection:** Store workspace-specific secrets (API keys, database URIs) that are injected into your scripts at runtime — including across the WSL boundary.
* **Local-First & High Performance:** Built on **Tauri v2 + Rust + SQLite** rather than Electron. Roughly a **40–80 MB idle RAM footprint**, **sub-millisecond queries** against an on-disk database, and total data sovereignty.
* **Data Portability:** Your entire setup exports to a single JSON file and imports back on any machine.

## ⚡ Why Not Electron?

|                      | Orion                                | Typical Electron app       |
| -------------------- | ------------------------------------ | -------------------------- |
| **Runtime**    | Native WebView2 (already on Windows) | Bundled Chromium           |
| **Idle RAM**   | ~40–80 MB                           | 300 MB+                    |
| **Data layer** | Embedded SQLite, sub-ms queries      | Cloud API or bundled DB    |
| **Your data**  | Stays in`%APPDATA%`, always        | Usually synced to a vendor |

No accounts. No telemetry. No cloud sync. The only network request Orion ever makes is the update check you trigger yourself from Settings — everything else works fully offline, forever.

## 📦 Installation (Beta)

1. Navigate to the [Releases](../../releases) page.
2. Download the latest `Orion_x64-setup.exe` file.
3. Run the installer.
   * *Note: As this is an indie beta release, Windows SmartScreen may show a blue warning. Click **More Info** -> **Run anyway**.*
4. Look for the Orion icon in your Windows System Tray to open the dashboard!

## 🛠️ Usage Quickstart

1. **Create a Workspace:** Set up a dedicated area for a single project — this is your one context.
2. **Add Resources:** Link your local folders, API documentation, or GitHub repos. Use the quick-launch icons to open a folder directly in your terminal or IDE.
3. **Track Your Tasks:** Keep the project's working checklist in the workspace instead of a scratch file you will lose.
4. **Start the Timer:** Run a stopwatch or a Pomodoro. Sessions are logged per workspace and roll up into Total Time, which you can reset whenever a new sprint starts.
5. **Write a Script:** Automate your data pipelines or environment setups.
6. **Hide the App:** Close the window. It keeps running in the background. Press `Ctrl + Shift + O` to summon it instantly.

Built with [Tauri v2](https://tauri.app), Rust, React, and SQLite.
