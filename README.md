# Orion 🚀

**A Context-Switching & Focus Workspace for Windows and macOS.**

Orion is a zero-friction, single-context workspace launcher that instantly loads your project’s essential resources, notes, and focus timer—eliminating tab clutter and keeping you locked into one task at a time.

It lives in your system tray — the menu bar on a Mac. Press `Ctrl + Shift + O` (`⌘⇧O` on macOS, rebindable in Settings), type a project name, and you are in: the right folders, the right docs, the right scripts, and a running timer. Nothing else competing for your attention.

## ✨ Key Features

* **Global Quick Search:** Press `Ctrl + Shift + O` (`⌘⇧O` on macOS) from anywhere — or rebind it to any combination you prefer — to search and launch instantly. Narrow the results with slash command scoping: `/ws` for workspaces, `/link` for links, `/folder` for folders, `/script` for automation scripts, and `/ai` to search by meaning rather than by exact words.
* **Multi-Language Automation:** Write and execute scripts directly from the UI, with per-workspace interpreter overrides. On Windows: PowerShell, CMD, WSL Bash, Python and Node.js. On macOS: zsh, bash, Python and Node.js. See [Scripts across platforms](#-scripts-across-platforms).
* **Focus Analytics:** Independent Pomodoro and stopwatch timers per workspace that keep running while you work elsewhere in the app, native OS notification banners on completion, and resettable **Total Time** tracking so you can measure a sprint without losing your history.
* **Environment Variable Injection:** Store workspace-specific secrets (API keys, database URIs) that are injected into your scripts at runtime — including across the WSL boundary.
* **Local-First & High Performance:** Built on **Tauri v2 + Rust + SQLite** rather than Electron. Roughly a **40–80 MB idle RAM footprint**, **sub-millisecond queries** against an on-disk database, and total data sovereignty.
* **Data Portability:** Your entire setup exports to a single JSON file and imports back on any machine, including one running the other operating system.

## ⚡ Why Not Electron?

|                      | Orion                                | Typical Electron app       |
| -------------------- | ------------------------------------ | -------------------------- |
| **Runtime**    | The OS web view (WebView2 on Windows, WebKit on macOS) | Bundled Chromium           |
| **Idle RAM**   | ~40–80 MB                           | 300 MB+                    |
| **Data layer** | Embedded SQLite, sub-ms queries      | Cloud API or bundled DB    |
| **Your data**  | Stays on your machine, always      | Usually synced to a vendor |

No accounts. No telemetry. No cloud sync. The only network request Orion ever makes is the update check you trigger yourself from Settings — everything else works fully offline, forever.

## 📦 Installation (Beta)

Both builds are unsigned — Orion is an indie project without a code-signing
certificate — so each operating system will warn you once. The steps below are
how you get past that. They are a one-time thing per install.

### Windows

1. Go to the [Releases](../../releases) page.
2. Download the latest `Orion_x64-setup.exe`.
3. Run the installer. SmartScreen may show a blue warning — click **More info**
   → **Run anyway**.
4. Look for the Orion icon in your system tray to open the dashboard.

### macOS (Apple Silicon)

Requires macOS 11 or later. Intel Macs are not currently built for.

1. Go to the [Releases](../../releases) page.
2. Download the latest `Orion_aarch64.dmg`.
3. Open it and drag **Orion** to your Applications folder.
4. **The first launch needs a right-click.** Find Orion in Applications,
   right-click (or Control-click) it, choose **Open**, then confirm **Open** in
   the dialog. Double-clicking it the normal way will be blocked; right-clicking
   is what gives you the option to proceed. You only need to do this once.
5. If instead you see **"Orion is damaged and can't be opened"**, macOS has
   quarantined the download. Clear the flag in Terminal:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Orion.app
   ```

   Then open it normally. This is Gatekeeper refusing an unsigned app, not a
   corrupted file.
6. Look for the Orion icon in your menu bar to open the dashboard.

**Updating on macOS** is manual: download the new `.dmg` and replace the app.
The in-app updater only serves Windows, because an unsigned app cannot reliably
replace itself — the downloaded archive gets quarantined and the swap fails.
Settings tells you this rather than reporting a network error.

## 🔀 Scripts across platforms

Your whole setup — workspaces, resources, scripts, timers, tasks and variables —
moves between a Windows machine and a Mac through **Settings → Data →
Export / Import**. Nothing is dropped or rewritten in the crossing.

Scripts are the one place where the two platforms genuinely differ:

| Script type | Windows | macOS |
| ----------- | :-----: | :---: |
| Python      |    ✅    |   ✅   |
| Node.js     |    ✅    |   ✅   |
| PowerShell  |    ✅    |   —   |
| CMD         |    ✅    |   —   |
| WSL Bash    |    ✅    |   —   |
| zsh         |    —    |   ✅   |
| bash        |    —    |   ✅   |

**Python and Node.js scripts work on both.** If most of your automation is in
one of those two, it comes across and simply runs.

**A script belonging to the other platform is kept, not deleted.** Import a
Windows setup on a Mac and your PowerShell scripts are all still there: listed,
readable and editable, with their type badge dimmed and the Run button disabled.
Hovering it explains why. They start working again the moment you open that same
setup back on Windows.

Orion tells you this once, at the point of import, rather than letting you find
it one script at a time. It deliberately does **not** try to translate anything:
a PowerShell script machine-converted to zsh would look plausible and be wrong,
which is worse than one that honestly declines to run.

> **macOS interpreter note.** An app launched from the Dock does not inherit the
> PATH your shell builds, so Orion adds the usual Homebrew and MacPorts
> locations when it runs a script. If you keep `python3` or `node` somewhere
> unusual, set an absolute path in **Settings → General**.

## 🛠️ Usage Quickstart

1. **Create a Workspace:** Set up a dedicated area for a single project — this is your one context.
2. **Add Resources:** Link your local folders, API documentation, or GitHub repos. Use the quick-launch icons to open a folder directly in your file manager, terminal or IDE.
3. **Track Your Tasks:** Keep the project's working checklist in the workspace instead of a scratch file you will lose.
4. **Start the Timer:** Run a stopwatch or a Pomodoro. Sessions are logged per workspace and roll up into Total Time, which you can reset whenever a new sprint starts.
5. **Write a Script:** Automate your data pipelines or environment setups.
6. **Hide the App:** Close the window. It keeps running in the background. Press `Ctrl + Shift + O` (`⌘⇧O` on macOS) to summon it instantly.

Built with [Tauri v2](https://tauri.app), Rust, React, and SQLite.
