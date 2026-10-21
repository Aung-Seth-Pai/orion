import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyTheme,
  DEFAULT_THEME,
  isThemeId,
  readCachedTheme,
  THEMES,
} from "./theme";

afterEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  vi.restoreAllMocks();
});

describe("isThemeId", () => {
  it("accepts every shipped theme", () => {
    for (const t of THEMES) expect(isThemeId(t.id)).toBe(true);
  });

  it("rejects anything else", () => {
    // A backup written by a newer version can name a theme this build has
    // never heard of, so this guard is what stops it reaching the DOM.
    for (const value of ["", "midnight", "GRAPHITE", null, undefined, 3, {}]) {
      expect(isThemeId(value)).toBe(false);
    }
  });
});

describe("readCachedTheme", () => {
  it("returns the default when nothing is cached", () => {
    expect(readCachedTheme()).toBe(DEFAULT_THEME);
  });

  it("returns a cached theme", () => {
    applyTheme("ember");
    expect(readCachedTheme()).toBe("ember");
  });

  it("falls back to the default when the cache holds an unknown theme", () => {
    localStorage.setItem("orion.theme", "midnight");
    expect(readCachedTheme()).toBe(DEFAULT_THEME);
  });

  it("falls back to the default when storage throws", () => {
    // Private windows and blocked site data make this throw rather than
    // return null. Failing to read a preference must never fail the render.
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage disabled");
    });
    expect(readCachedTheme()).toBe(DEFAULT_THEME);
  });
});

describe("applyTheme", () => {
  it("puts the theme on the document element", () => {
    applyTheme("almanac");
    expect(document.documentElement.dataset.theme).toBe("almanac");
  });

  it("still applies the theme when the cache write fails", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    expect(() => applyTheme("deep-field")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("deep-field");
  });
});

describe("the default theme", () => {
  it("is the look the app already shipped", () => {
    // Guards a real decision: changing this restyles every existing install
    // on update. It should take a deliberate edit here, not a drive-by.
    expect(DEFAULT_THEME).toBe("graphite");
  });
});
