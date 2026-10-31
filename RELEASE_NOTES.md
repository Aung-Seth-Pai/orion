Orion v0.5.0 — Backups can be encrypted.

Install over your existing copy. Your data, workspaces, resources, scripts and timers are untouched.

## Your backup no longer has to be readable

**Settings → Data → Export data backup** now offers you a password, and offers it before anything is written.

The reason is the environment variables. A backup has always included their values — the API keys and database URIs you store per workspace — and it wrote them in clear text, into exactly the kind of file that ends up on a USB stick, in a sync folder, or attached to an email. Anyone who held the file held the keys.

With a password, the file is encrypted with **AES-256-GCM**, and the key is derived from your password using **Argon2id**, deliberately tuned to take about half a second. You notice that once; someone trying millions of guesses notices it a great deal more.

Beyond confidentiality, the file now carries an authentication tag covering both the data and the encryption settings. A backup that has been altered, truncated or damaged in transit is refused outright, rather than restoring part of itself and leaving you to work out which part.

## It still crosses between Windows and macOS

An encrypted backup exported on a Mac imports on Windows, and the other way round. The settings needed to derive the key travel inside the file, so it also opens on a computer faster or slower than the one that wrote it.

The file is still a `.json` file, so every sync tool, file picker and save dialog behaves exactly as before.

## Smaller things

- Choosing a backup file now tells Orion whether it needs a password, so you are asked up front rather than after a failure.
- A wrong password asks again instead of making you pick the file a second time, and nothing in your current setup is touched until the password is right.
- Exporting without a password is still available, and says plainly what it means rather than leaving you to infer it.
- Backups written by v0.4.0 and earlier import unchanged.

## One thing to be clear about

**A lost password cannot be recovered.** There is no reset, no recovery code, and no key held anywhere else — which is precisely what makes the encryption worth having. Keep it in a password manager.

## Coming

macOS support is built and is being tested on real hardware. It is not in this release. When it lands, the same backup file — encrypted or not — will carry a setup between a Windows machine and a Mac in either direction, with anything the other platform cannot run kept intact rather than dropped.

## Upgrading

Install over your existing copy, or let the in-app updater offer it to you.

---

*Windows SmartScreen may warn on this installer — it is an indie build without a code-signing certificate. Click **More info** → **Run anyway**.*
