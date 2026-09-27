import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

/**
 * The running app's version, read from the Tauri bundle rather than written
 * into the UI by hand. The two places that display it previously carried a
 * hardcoded string, which drifted the moment a release was cut: the app
 * shipped as 0.1.2 while the sidebar and status bar both still claimed 0.1.0.
 *
 * Returns null until the call resolves, so callers render nothing rather than
 * a wrong number for a frame.
 */
export function useAppVersion(): string | null {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((v) => {
        if (!cancelled) setVersion(v);
      })
      // Not worth surfacing: a missing version label is cosmetic, and the
      // caller already renders nothing while it is null.
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  return version;
}
