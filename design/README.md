# Design lab

Static pages for Orion's visual identity. **Open them directly in a browser** —
no build step, no dev server, nothing to install:

| Page | What it is |
|---|---|
| `themes.html` | The four themes, one composition, side by side |
| `marks.html` | Constellation marks: enlarged geometry, the size they are used at, and the time ramp |

`themes.html` links `../src/styles/tokens.css` rather than copying it, so the
guide cannot drift from what ships.

## The contract

**A theme changes values, never markup.** The four apps on `themes.html` are the
same HTML with a different `data-theme` on the wrapper; no component anywhere is
allowed to branch on which theme is active.

That rule is what keeps four themes from costing four times the maintenance, and
it doubles as a test: switch themes and any surface that fails to change still
has a hardcoded colour in it. If a design idea cannot be expressed as a value in
`tokens.css`, it does not become a theme — it becomes a change to the one shared
structure, applied to all of them.

Three ideas were dropped to satisfy it, and each was an improvement anyway:

- **Ember had no Launch button** and acted on a row click. That gesture is
  already taken by copy-to-clipboard, so the button stays in every theme.
- **Ember spelled the workspace count as "twelve."** Charming; breaks at a
  hundred workspaces and in every other language.
- **Almanac used a `↗` glyph** where the others used a labelled control.

## Fixtures

`themes.html?fixture=<name>` swaps the data. These are deliberately **not** one
developer's real workspaces — a design calibrated against eight tidy Latin names
between 2 and 41 hours breaks the first time it meets somebody else's data.

| Fixture | What it is for |
|---|---|
| `typical` | Eight mixed workspaces (default) |
| `fresh` | Brand new install, every workspace at zero hours |
| `single` | One workspace, half an hour |
| `heavy` | Ten years of use, up to 2410 hours |
| `hostile` | Burmese, Japanese, Korean, Arabic, Cyrillic, emoji, a 70-character name, a one-character name |

Two bugs came out of the hostile set that no amount of looking at Latin names
would have found: tall scripts clipped against a tight `line-height`, and
right-to-left names laid out as though they were English. Every element that
renders a user-supplied name now sets an explicit line height and `dir="auto"`.

## Rendering to an image

Useful for comparing revisions without a browser open. `?theme=` renders one
theme on its own, which is what to use when the type needs to be legible —
all four at a readable scale is too tall to screenshot in one go.

```bash
chrome --headless --disable-gpu --hide-scrollbars \
  --force-device-scale-factor=1.6 --virtual-time-budget=4000 \
  --screenshot=out.png --window-size=960,580 \
  "file:///C:/Users/zepha/workspace/orion/design/themes.html?theme=ember&fixture=hostile"
```

`--virtual-time-budget` matters: without it the screenshot fires before the
webfonts load and Ember renders in a fallback face.

## `mark.js`

The constellation generator, kept framework-free so both this folder and the app
can use it. A workspace is hashed to seed a PRNG, points are placed on a
jittered grid, and a minimum spanning tree connects them — so the same workspace
always produces the same figure, and no edges needlessly cross.

Four things in here are less obvious than they look:

- **Seeded by workspace ID, not name.** Seeding by name means renaming a
  workspace silently replaces a figure you had learned to recognise.
- **`depthFromHours` calibrates against the user's own busiest workspace**, not
  an absolute constant. An absolute ceiling is the obvious implementation and it
  is wrong in both directions: pick 45 hours and a casual user sees twelve
  identical floor-level marks forever, while someone with two thousand hours
  sees twelve identical saturated ones. The ceiling is floored at 20 hours so a
  fresh install does not crown whichever workspace was opened first.
- **Shape and investment are separate meanings.** The figure is always drawn in
  full; only its brightness scales with time. An earlier version withheld the
  lines until a workspace had hours on it so the figure would "emerge" with use.
  It reads nicely, and rendering the `fresh` fixture killed it: three brand new
  workspaces came out as three near-identical scatters of dots, at exactly the
  moment a new user most needs to tell them apart.
- **Optical sizing.** The viewBox is always 100 units, so a fixed stroke width
  renders a quarter of a pixel at 22px and a clumsy slab at 86px. Weights are
  specified in *rendered* pixels and converted, which is why `constellation()`
  takes the display size.
- **Partial fill.** Stretching each figure fully to its bounding box evens out
  optical weight in a list, but snaps too many of them into axis-aligned Ls and
  brackets that read as geometry rather than sky. `fill` pulls most of the way
  and stops short.
