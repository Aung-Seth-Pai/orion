# Orion 🚀

**A Context-Switching & Focus Workspace for Windows and macOS.**

Orion is a zero-friction, single-context workspace launcher that instantly loads your project’s essential resources, notes, and focus timer—eliminating tab clutter and keeping you locked into one task at a time.

It lives in your system tray — the menu bar on a Mac. Press `Ctrl + Shift + O` (`⌥⌘O` on macOS, rebindable in Settings), type a project name, and you are in: the right folders, the right docs, the right scripts, and a running timer. Nothing else competing for your attention.

## ✨ Key Features

* **Global Quick Search:** Press `Ctrl + Shift + O` (`⌥⌘O` on macOS) from anywhere — or rebind it to any combination you prefer — to search and launch instantly. Narrow the results with slash command scoping: `/ws` for workspaces, `/link` for links, `/folder` for folders, `/script` for automation scripts, and `/ai` to search by meaning rather than by exact words.
* **Multi-Language Automation:** Write and execute scripts directly from the UI, with per-workspace interpreter overrides. On Windows: PowerShell, CMD, WSL Bash, Python and Node.js. On macOS: zsh, bash, Python and Node.js. See [Scripts across platforms](#-scripts-across-platforms).
* **Focus Analytics:** Independent Pomodoro and stopwatch timers per workspace that keep running while you work elsewhere in the app, native OS notification banners on completion, and resettable **Total Time** tracking so you can measure a sprint without losing your history.
* **Environment Variable Injection:** Store workspace-specific secrets (API keys, database URIs) that are injected into your scripts at runtime — including across the WSL boundary.
* **Local-First & High Performance:** Built on **Tauri v2 + Rust + SQLite**. Roughly a **40–80 MB idle RAM footprint** and **sub-millisecond queries** against an on-disk database.
* **Offline & Private:** No accounts, no telemetry, no cloud sync. The only network request Orion makes is the update check you trigger yourself from Settings.
* **Data Portability:** Your entire setup exports to a single file and imports back on any machine, including one running the other operating system. The export can be **encrypted with a password** — see [Backups](#-backups).

## 📦 Installation (Beta)

Both builds are unsigned, so each operating system warns you once. The steps
below clear that warning. You do this once per install.

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
3. Open it and drag **Orion** to your Applications folder. Do this before the
   next step — clearing the quarantine flag on the copy still inside the
   mounted disk image does not carry over to the one you install.
4. **The first launch is blocked.** You will see *"Apple could not verify
   'Orion' is free of malware"*, offering only **Move to Trash** or **Cancel**.
   Click **Cancel** — the download is fine; macOS blocks any app that has not
   been notarized. Then allow it once, either way:

   **In Terminal:**

   ```sh
   xattr -dr com.apple.quarantine /Applications/Orion.app
   ```

   Then open Orion normally.

   **Or without Terminal:** open **System Settings → Privacy & Security** and
   scroll to the **Security** section. A line reading *"Orion was blocked to
   protect your Mac"* appears there with an **Open Anyway** button. Click it,
   authenticate, and confirm **Open**. That line only shows for a few minutes
   after a blocked launch, so if it is missing, try opening Orion again first.

   Either way, you only do this once. Subsequent launches are normal.

   > On macOS 14 and earlier you could instead right-click the app and choose
   > **Open**. Apple removed that shortcut in macOS 15 (Sequoia); on current
   > versions, use one of the two methods above.
5. Look for the Orion icon in your menu bar to open the dashboard.

**Updating on macOS** is manual: download the new `.dmg` and replace the app.
The in-app updater serves Windows only.

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
readable and editable, with their type badge dimmed and the Run button
disabled. Hovering it explains why. They start working again the moment you
open that same setup back on Windows.

Scripts are never translated between platforms. The import summary lists any
that cannot run on the machine you imported onto.

> **macOS interpreter note.** An app launched from the Dock does not inherit the
> PATH your shell builds, so Orion adds the usual Homebrew and MacPorts
> locations when it runs a script. If you keep `python3` or `node` somewhere
> unusual, set an absolute path in **Settings → General**.

## 🔐 Backups

**Settings → Data → Export data backup** writes your whole setup to one file and
offers you a password. A backup includes the values of your **environment
variables**, so without a password those values are readable by anyone who
opens the file.

With a password, the file is encrypted with **AES-256-GCM** and the key is
derived using **Argon2id**, which takes about half a second. A backup that has
been altered or damaged is refused rather than half-restored.

The file is a `.json` file either way, so sync tools, file pickers and save
dialogs behave the same. An encrypted backup exported on a Mac imports on
Windows and the other way round, and contains nothing that identifies you, your
machine or when it was made.

> **A lost password cannot be recovered.** There is no reset and no recovery
> code. Keep it in a password manager.

Importing replaces everything currently in Orion. A wrong password fails before
any of your data is touched and asks again, without making you choose the file
a second time.

## 🛠️ Usage Quickstart

1. **Create a Workspace:** Set up a dedicated area for a single project — this is your one context.
2. **Add Resources:** Link your local folders, API documentation, or GitHub repos. Use the quick-launch icons to open a folder directly in your file manager, terminal or IDE.
3. **Track Your Tasks:** Keep the project's working checklist in the workspace instead of a scratch file you will lose.
4. **Start the Timer:** Run a stopwatch or a Pomodoro. Sessions are logged per workspace and roll up into Total Time, which you can reset whenever a new sprint starts.
5. **Write a Script:** Automate your data pipelines or environment setups.
6. **Hide the App:** Close the window. It keeps running in the background. Press `Ctrl + Shift + O` (`⌥⌘O` on macOS) to summon it instantly.

Built with [Tauri v2](https://tauri.app), Rust, React, and SQLite.
