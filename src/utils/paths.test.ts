import { describe, expect, it } from "vitest";
import { displayTarget } from "./paths";

describe("displayTarget", () => {
  it("turns a drive-letter file URL back into a native path", () => {
    expect(displayTarget("file:///C:/Users/zepha/Downloads/Resume.pdf")).toBe(
      "C:\\Users\\zepha\\Downloads\\Resume.pdf"
    );
  });

  it("decodes the escapes the normaliser introduced", () => {
    // Round-trips what the Rust side produces for `C:\My Docs\draft #2.pdf`.
    expect(displayTarget("file:///C:/My%20Docs/draft%20%232.pdf")).toBe(
      "C:\\My Docs\\draft #2.pdf"
    );
  });

  it("restores the leading slashes of a UNC path", () => {
    expect(displayTarget("file://nas/share/spec.pdf")).toBe(
      "\\\\nas\\share\\spec.pdf"
    );
  });

  it("leaves web URLs alone", () => {
    expect(displayTarget("https://example.com/a.pdf")).toBe(
      "https://example.com/a.pdf"
    );
    // A %20 in a web URL is part of the URL and must not be decoded.
    expect(displayTarget("https://example.com/a%20b.pdf")).toBe(
      "https://example.com/a%20b.pdf"
    );
  });

  it("leaves folder paths alone", () => {
    expect(displayTarget("C:\\src\\payments")).toBe("C:\\src\\payments");
  });

  it("matches the scheme case-insensitively", () => {
    expect(displayTarget("FILE:///C:/tmp/a.txt")).toBe("C:\\tmp\\a.txt");
  });

  it("falls back to the raw value on a malformed escape", () => {
    // decodeURIComponent throws on a lone '%'; showing it raw beats crashing
    // the card it is rendered in.
    const malformed = "file:///C:/bad%path";
    expect(displayTarget(malformed)).toBe(malformed);
  });
});
