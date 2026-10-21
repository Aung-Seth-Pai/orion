/**
 * Turns a `file://` URL back into the native path it came from.
 *
 * A local file resource is stored as a URL because that is what the launcher
 * needs, but the user typed a path and a path is what is useful to read and to
 * paste — `file:///C:/My%20Docs/cv.pdf` is worse on both counts than
 * `C:\My Docs\cv.pdf`. Anything that is not a `file://` URL is returned
 * unchanged, so http(s) links and folder paths pass straight through.
 *
 * The shape of the URL decides which form it came from, rather than the host
 * deciding: a drive letter or an authority means a Windows path and gets
 * backslashes, and anything else keeps the slashes it already has. So a backup
 * written on one platform and opened on the other displays each path the way
 * the machine it belongs to writes it, instead of showing `\Users\me\cv.pdf`
 * for a file that lives on a Mac.
 */
export function displayTarget(target: string): string {
  if (!target.toLowerCase().startsWith("file://")) return target;

  const withoutScheme = target.slice("file://".length);
  let decoded: string;
  try {
    decoded = decodeURIComponent(withoutScheme);
  } catch {
    // A malformed escape sequence is not worth failing over; show it raw.
    return target;
  }

  // file:///C:/... — the empty authority leaves a leading slash before the
  // drive letter that has no place in a native path. A drive letter is proof
  // this is a Windows path whatever platform is reading it.
  if (/^\/[A-Za-z]:/.test(decoded)) {
    return decoded.slice(1).replace(/\//g, "\\");
  }

  // file://server/share/... — a real authority, so this is a UNC path.
  if (decoded && !decoded.startsWith("/")) {
    return "\\\\" + decoded.replace(/\//g, "\\");
  }

  // file:///Users/me/... — a POSIX path. Its separators are already correct
  // for the machine it refers to, on whichever machine it is being read.
  return decoded;
}
