import { useCallback, useState } from "react";

/**
 * Moves the item at `from` to index `to`, returning a new array.
 *
 * Pure and exported separately from the hook so the index arithmetic — the part
 * that is easy to get subtly wrong when dragging downwards — can be tested on
 * its own.
 */
export function moveItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0) return items;
  if (from >= items.length || to >= items.length) return items;

  const next = items.slice();
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

interface DragReorder {
  /** The item currently being dragged, for dimming it. */
  draggingId: string | null;
  /** The item the pointer is over, for showing where the drop will land. */
  overId: string | null;
  /** Spread onto each list item to make it both draggable and a drop target. */
  itemProps: (id: string) => {
    draggable: true;
    onDragStart: (e: React.DragEvent) => void;
    onDragEnter: () => void;
    onDragOver: (e: React.DragEvent) => void;
    onDrop: (e: React.DragEvent) => void;
    onDragEnd: () => void;
  };
}

/**
 * Native HTML5 drag-and-drop reordering for a list of `{ id }` items.
 *
 * Deliberately no drag-and-drop library: the lists here are short, the browser
 * already provides everything needed, and this works unchanged for the resource
 * grid, where the drop target is whichever card the pointer is over rather than
 * a position in a single column.
 *
 * `onCommit` receives the reordered array and the id that moved. Callers apply
 * it optimistically and roll back if the backend refuses — the same pattern the
 * delete handlers use.
 *
 * `canDrop` rejects a pairing before anything moves, so an invalid target shows
 * no drop indicator either. The script list needs it: it renders a workspace's
 * scripts and the global ones as one list, but they are two separately ordered
 * scopes in the database and a drag may not cross between them.
 */
export function useDragReorder<T extends { id: string }>(
  items: T[],
  onCommit: (ordered: T[], movedId: string) => void,
  canDrop?: (sourceId: string, targetId: string) => boolean
): DragReorder {
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);

  const itemProps = useCallback(
    (id: string) => ({
      draggable: true as const,
      onDragStart: (e: React.DragEvent) => {
        setDraggingId(id);
        // Firefox ignores a drag with no data attached, and "move" gives the
        // correct cursor rather than the copy-plus badge.
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", id);
      },
      onDragEnter: () => {
        if (!draggingId || draggingId === id) return;
        if (canDrop && !canDrop(draggingId, id)) return;
        setOverId(id);
      },
      // Required: without preventDefault the browser refuses the drop.
      onDragOver: (e: React.DragEvent) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
      },
      onDrop: (e: React.DragEvent) => {
        e.preventDefault();
        const sourceId = draggingId ?? e.dataTransfer.getData("text/plain");
        setDraggingId(null);
        setOverId(null);
        if (!sourceId || sourceId === id) return;
        if (canDrop && !canDrop(sourceId, id)) return;

        const from = items.findIndex((item) => item.id === sourceId);
        const to = items.findIndex((item) => item.id === id);
        if (from === -1 || to === -1) return;

        onCommit(moveItem(items, from, to), sourceId);
      },
      onDragEnd: () => {
        setDraggingId(null);
        setOverId(null);
      },
    }),
    [draggingId, items, onCommit, canDrop]
  );

  return { draggingId, overId, itemProps };
}
