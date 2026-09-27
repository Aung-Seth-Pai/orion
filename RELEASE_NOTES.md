Orion v0.2.0 — semantic search, and a round of fixes to the things that got in the way daily.

## Search by meaning

Type `/ai` in the search window to find things by what they mean rather than what they are called — "deploy my app live" will surface a script named "Ship the release to production".

It is **off until you turn it on**, in Settings → AI Search. Enabling it downloads a ~90 MB model once; after that everything runs on your CPU, offline, forever. No text you store or search ever leaves your machine — the only network request is that one download. You can remove the model and clear the index from the same panel whenever you want the space back.

## Fixes

- **The window fits your screen now.** On a scaled display the old fixed size could be taller than the usable screen, so the app opened with its edges off-screen. It is now measured against the monitor it opens on, and it remembers the size and position you chose.
- **Local files can be added as links.** Paste an absolute path — `C:\Users\you\file.pdf`, exactly what Explorer's "Copy as path" gives you — instead of hand-writing a `file://` URL. Pair it with Chrome to open PDFs where you want them.
- **Timer notifications you can actually see.** Windows can accept a notification and still never show the banner, with no way for an app to tell. Orion now shows its own small bar in the corner, which no notification setting can suppress.
- **The version in the corner is real.** It was hardcoded and had drifted; it now reads from the build.

## Additions

- **Drag to reorder** workspaces, resources and scripts. The order is yours, it survives restarts, and it is preserved through export and import.
- **Click a resource to copy** its link or path, without launching it.
- **Rebind the global shortcut.** `Ctrl+Shift+O` is only the default now. If a combination is already taken by another app, Orion says so and keeps the one that works.
- **Duplicate detection while adding a resource.** As you type a name, Orion shows anything similar you already saved — across *all* your workspaces, since that is exactly when you would not remember. Paste a target you already have and it tells you where it lives.

## Upgrading

Your database migrates automatically the first time you launch; nothing to do. Semantic search stays off until you enable it.

One thing worth knowing: **importing a backup clears the AI index**, because an import replaces the whole database. If you use `/ai`, rebuild the index afterwards in Settings → AI Search. Orion will tell you if you forget.

---

*Windows SmartScreen may warn on this installer — it is an indie build without a code-signing certificate. Click **More info** → **Run anyway**.*
