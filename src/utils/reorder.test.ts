import { describe, expect, it } from "vitest";
import { moveItem } from "./reorder";

const list = [{ id: "a" }, { id: "b" }, { id: "c" }, { id: "d" }];
const ids = (items: { id: string }[]) => items.map((i) => i.id);

describe("moveItem", () => {
  it("moves an item downwards to the target's position", () => {
    // Dragging downwards is the case that is easy to get off by one, because
    // removing the item first shifts every later index.
    expect(ids(moveItem(list, 0, 2))).toEqual(["b", "c", "a", "d"]);
  });

  it("moves an item upwards", () => {
    expect(ids(moveItem(list, 3, 1))).toEqual(["a", "d", "b", "c"]);
  });

  it("moves to either end", () => {
    expect(ids(moveItem(list, 2, 0))).toEqual(["c", "a", "b", "d"]);
    expect(ids(moveItem(list, 0, 3))).toEqual(["b", "c", "d", "a"]);
  });

  it("returns the same array when nothing would change", () => {
    // Identity is checked, not just equality: a no-op must not trigger a
    // re-render or a pointless backend write.
    expect(moveItem(list, 1, 1)).toBe(list);
  });

  it("ignores out-of-range indices instead of corrupting the list", () => {
    expect(moveItem(list, -1, 2)).toBe(list);
    expect(moveItem(list, 0, 99)).toBe(list);
    expect(moveItem(list, 99, 0)).toBe(list);
  });

  it("does not mutate the input", () => {
    const original = [...list];
    moveItem(list, 0, 3);
    expect(list).toEqual(original);
  });

  it("preserves every item, so a reorder can never drop one", () => {
    // The backend rejects a list that does not match what is stored, so losing
    // an item here would surface as a confusing refusal rather than a bad order.
    const reordered = moveItem(list, 1, 3);
    expect(reordered).toHaveLength(list.length);
    expect(ids(reordered).sort()).toEqual(ids(list).sort());
  });
});
