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

export default function Spotlight() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selected, setSelected] = useState(0);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

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
    <div className="flex h-full flex-col rounded-xl border border-zinc-700/70 bg-zinc-900/90 shadow-2xl shadow-black/60 backdrop-blur-xl">
      <div className="flex items-center gap-3 border-b border-zinc-800 px-4">
        <Search size={18} className="shrink-0 text-zinc-500" />
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          maxLength={100}
          placeholder="Search workspaces, links, folders, scripts…"
          className="h-14 w-full bg-transparent text-[15px] text-zinc-100 placeholder-zinc-500 outline-none"
        />
        <kbd className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-500">
          Esc
        </kbd>
      </div>

      <div className="px-4 pt-1.5 text-[10px] text-zinc-600">
        Tip: Filter results using /ws, /link, /folder, /script, or /ai
      </div>

      <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto p-2 pt-1">
        {results.length === 0 ? (
          <li className="px-3 py-6 text-center text-xs text-zinc-600">
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
                      ? "bg-indigo-500/20 ring-1 ring-indigo-500/40"
                      : "hover:bg-zinc-800/60"
                  }`}
                >
                  <span className="flex size-7 shrink-0 items-center justify-center rounded bg-zinc-800 text-zinc-400">
                    <Icon size={14} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-medium text-zinc-200">
                      {item.title}
                    </span>
                    <span className="block truncate text-[11px] text-zinc-500">
                      {item.subtitle}
                    </span>
                  </span>
                  <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-zinc-500">
                    {item.itemType}
                  </span>
                </button>
              </li>
            );
          })
        )}
      </ul>

      <div className="flex items-center gap-3 border-t border-zinc-800 px-4 py-1.5 text-[10px] text-zinc-600">
        <span>↑↓ navigate</span>
        <span>↵ open</span>
        <span>esc dismiss</span>
        <span className="ml-auto">Ctrl+Shift+O</span>
      </div>
    </div>
  );
}
