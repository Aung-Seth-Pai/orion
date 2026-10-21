Orion v0.2.1 — a fix for new installs. **If you installed v0.2.0, please update.**

## The problem

On a fresh install, the main window opened empty with a red banner reading *"state not managed for field `db` on command `get_workspaces`"*, and no workspaces appeared. Quick search still found everything, and importing a backup made the banner disappear — which made it look intermittent.

It was a startup race. Tauri creates the app's windows *before* the setup code that opens the database runs, so on a first launch the interface came up and asked for your workspaces while the database was still being created and migrated. An existing install never hit it, because there is no migration work to do and the database wins the race.

Windows are now created only after the database is ready, so there is no longer a window in which the question can be asked too early.

Two smaller fixes came out of the same investigation:

- An error banner could outlive the problem that caused it. A successful load now clears it, instead of leaving a stale message until some unrelated action happened to reset it.
- When another application already owns `Ctrl+Shift+O`, Orion logged an alarming error and tried to claim the same combination a second time. It now says so once, calmly, and you can set a different shortcut in Settings.

## Upgrading

Install over your existing copy. Your data is untouched, and nothing needs to be re-done — if you were already running v0.2.0 successfully, you were never affected by this.

Everything in [v0.2.0](../../releases/tag/v0.2.0) — semantic search, drag to reorder, local file links, the rebindable shortcut, duplicate detection — is unchanged.

---

*Windows SmartScreen may warn on this installer — it is an indie build without a code-signing certificate. Click **More info** → **Run anyway**.*
