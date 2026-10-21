import { useCallback, useEffect, useRef, useState } from "react";
import {
  Folder,
  Globe,
  LayoutGrid,
  ScrollText,
  Search,
  Sparkles,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { getSettings } from "../api/settings";
import { searchAll } from "../api/search";
import { executeScript } from "../api/scripts";
import { launchResource } from "../api/resources";
import type { SearchResult } from "../types";

const TYPE_ICONS: Record<SearchResult["itemType"], typeof Globe> = {
  workspace: LayoutGrid,
  link: Globe,
  folder: Folder,
  script: ScrollText,
  notice: Sparkles,
};

const DEFAULT_SPOTLIGHT_SHORTCUT = "CmdOrCtrl+Shift+O";

/** `CmdOrCtrl+Shift+O` -> `Ctrl+Shift+O` for display. */
function prettifyShortcut(accelerator: string): string {
  return accelerator.replace("CmdOrCtrl", "Ctrl");
}

export default function Spotlight() {
  const [query, setQuery] = useState("");
  const [shortcut, setShortcut] = useState(DEFAULT_SPOTLIGHT_SHORTCUT);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const [summons, setSummons] = useState(0);

  // The spotlight window is hidden and re-shown rather than created and
  // destroyed, so this component mounts once and a mount animation would play
  // exactly one time -- on the first summon of the session, and never again.
  // Bumping a key on focus replays it each time the window comes forward.
  useEffect(() => {
    function onFocus() {
      setSummons((n) => n + 1);
    }
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, []);

  // Focusing has to happen HERE rather than in the listener above. The key
  // bump remounts the panel, so an input focused inside the event handler is
  // the one React is about to throw away -- the summon would animate nicely
  // and then leave the user typing into nothing. Running it as an effect on
  // `summons` puts it after the remount, on the element that survives.
  useEffect(() => {
    inputRef.current?.focus();
  }, [summons]);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults([]);
      setSelected(0);
      setLoading(false);
      return;
    }
    setLoading(true);
    const timer = window.setTimeout(() => {
      searchAll(q)
        .then((r) => {
          setResults(r);
          setSelected(0);
        })
        .catch(() => {})
        .finally(() => setLoading(false));
    }, 140);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    listRef.current
      ?.querySelectorAll("li")
      [selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  useEffect(() => {
    let cancelled = false;
    getSettings()
      .then((s) => {
        const stored = s["spotlight_shortcut"]?.trim();
        if (!cancelled && stored) setShortcut(stored);
      })
      // A stale hint is cosmetic; the default is already shown.
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const hide = useCallback(() => {
    void invoke("hide_spotlight").catch(() => {});
    setQuery("");
    setResults([]);
    setSelected(0);
  }, []);

  const execute = useCallback(
    async (item: SearchResult) => {
      try {
        switch (item.action) {
          case "execute_script":
            // Fire and forget — long-running scripts must not block hiding.
            void executeScript(item.id).catch(() => {});
            break;
          case "launch_resource":
            await launchResource(item.id);
            break;
          case "open_workspace":
            await emitTo("main", "orion-select-workspace", item.id);
            await invoke("show_main");
            break;
          case "none":
            // The "enable semantic search" notice. It launches nothing, but
            // surfacing the main window puts Settings one click away.
            await invoke("show_main");
            break;
        }
      } catch {
        // Errors are non-fatal here; the spotlight always hides.
      }
      hide();
    },
    [hide]
  );

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelected((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = results[selected];
      if (item) void execute(item);
    } else if (e.key === "Escape") {
      e.preventDefault();
      hide();
    }
  }

  return (
    <div
      key={summons}
      className="orion-summon flex h-full flex-col rounded-xl border border-rule-strong/70 bg-surface/90 shadow-2xl shadow-black/60 backdrop-blur-xl"
    >
      <div className="flex items-center gap-3 border-b border-rule px-4">
        <Search size={18} className="shrink-0 text-ink-faint" />
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          maxLength={100}
          placeholder="Search workspaces, links, folders, scripts…"
          className="h-14 w-full bg-transparent text-[15px] text-ink-strong placeholder-ink-faint outline-none"
        />
        <kbd className="shrink-0 rounded bg-raised px-1.5 py-0.5 text-[10px] text-ink-faint">
          Esc
        </kbd>
      </div>

      <div className="px-4 pt-1.5 text-[10px] text-ink-faint">
        Tip: Filter results using /ws, /link, /folder, /script, or /ai
      </div>

      <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto p-2 pt-1">
        {results.length === 0 ? (
          <li className="px-3 py-6 text-center text-xs text-ink-faint">
            {query.trim()
              ? loading
                ? "Searching…"
                : "No matches found"
              : "Type to search across Orion"}
          </li>
        ) : (
          results.map((item, index) => {
            const Icon = TYPE_ICONS[item.itemType];
            return (
              <li key={`${item.action}-${item.id}`}>
                <button
                  onMouseEnter={() => setSelected(index)}
                  onClick={() => void execute(item)}
                  className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors cursor-default ${
                    index === selected
                      ? "bg-accent/20 ring-1 ring-accent/40"
                      : "hover:bg-raised/60"
                  }`}
                >
                  <span className="flex size-7 shrink-0 items-center justify-center rounded bg-raised text-ink-muted">
                    <Icon size={14} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium text-ink">
                      {item.title}
                    </span>
                    <span className="block truncate text-[11px] text-ink-faint">
                      {item.subtitle}
                    </span>
                  </span>
                  <span className="shrink-0 rounded bg-raised px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-ink-faint">
                    {item.itemType}
                  </span>
                </button>
              </li>
            );
          })
        )}
      </ul>

      <div className="flex items-center gap-3 border-t border-rule px-4 py-1.5 text-[10px] text-ink-faint">
        <span>↑↓ navigate</span>
        <span>↵ open</span>
        <span>esc dismiss</span>
        <span className="ml-auto">{prettifyShortcut(shortcut)}</span>
      </div>
    </div>
  );
}
