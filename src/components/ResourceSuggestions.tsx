import { AlertTriangle, Folder, Globe, Sparkles } from "lucide-react";
import type { ResourceSuggestion } from "../types";
import { displayTarget } from "../utils/paths";

interface ResourceSuggestionsProps {
  /** A resource already pointing at the same target, if one exists. */
  duplicate: ResourceSuggestion | null;
  /** Resources merely resembling what is being typed. */
  similar: ResourceSuggestion[];
  /** Jumps to the workspace holding a suggestion. */
  onOpen: (suggestion: ResourceSuggestion) => void;
}

/**
 * Tells you whether you have already saved this, while you are typing it.
 *
 * An exact target match and a resemblance are deliberately presented as
 * different things: the first is a statement of fact worth a warning, the second
 * is a hint worth a glance. Collapsing them into one list would either
 * over-alarm on a coincidence or under-sell a real duplicate.
 */
export default function ResourceSuggestions({
  duplicate,
  similar,
  onOpen,
}: ResourceSuggestionsProps) {
  if (!duplicate && similar.length === 0) return null;

  return (
    <div className="col-span-2 -mt-1 space-y-1.5">
      {duplicate && (
        <button
          onClick={() => onOpen(duplicate)}
          className="flex w-full items-start gap-2 rounded border border-amber-500/30 bg-amber-500/10 px-2.5 py-2 text-left text-xs leading-relaxed text-amber-200/90 transition-colors hover:border-amber-500/50"
        >
          <AlertTriangle size={13} className="mt-0.5 shrink-0" />
          <span className="min-w-0">
            You already saved this as{" "}
            <span className="font-medium">{duplicate.title}</span> in{" "}
            <span className="font-medium">{duplicate.workspaceName}</span>.
          </span>
        </button>
      )}

      {similar.length > 0 && (
        <div className="overflow-hidden rounded border border-zinc-700/60 bg-zinc-900/80">
          <p className="border-b border-zinc-800 px-2.5 py-1.5 text-[10px] font-semibold tracking-wider text-zinc-500 uppercase">
            Already saved something similar?
          </p>
          <ul>
            {similar.map((item) => (
              <li key={item.id}>
                <button
                  onClick={() => onOpen(item)}
                  className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-zinc-800/60"
                >
                  <span
                    className={`flex size-5 shrink-0 items-center justify-center rounded ${
                      item.type === "link"
                        ? "bg-indigo-500/15 text-indigo-400"
                        : "bg-emerald-500/15 text-emerald-400"
                    }`}
                  >
                    {item.type === "link" ? (
                      <Globe size={11} />
                    ) : (
                      <Folder size={11} />
                    )}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs text-zinc-200">
                      {item.title}
                    </span>
                    <span className="block truncate text-[10px] text-zinc-500">
                      {displayTarget(item.targetPath)}
                    </span>
                  </span>
                  <span className="shrink-0 rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
                    {item.workspaceName}
                  </span>
                  {/* Marks the ones plain text matching could not have found. */}
                  {item.matchKind === "semantic" && (
                    <span title="Found by meaning" className="shrink-0">
                      <Sparkles size={11} className="text-indigo-400" />
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
