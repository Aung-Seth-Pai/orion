# Design lab

Static pages for working on Orion's visual identity. **Open them directly in a
browser** — no build step, no dev server, nothing to install:

| Page | What it is |
|---|---|
| `treatments.html` | The three candidate directions, side by side, with real workspace names |
| `marks.html` | Constellation marks: enlarged geometry, the size they are actually used at, and the time ramp |

They are plain HTML and inline styles on purpose. Once a direction is chosen this
folder becomes the living style guide — tokens, type scale, states — and the
inline styles are replaced by the same `tokens.css` the app uses, so the guide
cannot drift from what ships.

## Rendering to an image

Useful for comparing revisions without a browser open:

```bash
chrome --headless --disable-gpu --hide-scrollbars \
  --force-device-scale-factor=2 --virtual-time-budget=3000 \
  --screenshot=out.png --window-size=980,1800 \
  "file:///C:/Users/zepha/workspace/orion/design/treatments.html"
```

`--virtual-time-budget` matters: without it the screenshot fires before the
webfonts load and the serif treatment renders in a fallback face.

## `mark.js`

The constellation generator, kept framework-free so both this folder and the app
can use it. A workspace name is hashed to seed a PRNG, points are placed on a
jittered grid, and a minimum spanning tree connects them — so the same name
always produces the same figure, and no edges needlessly cross.

Two things in here are less obvious than they look:

- **Optical sizing.** The viewBox is always 100 units, so a fixed stroke width
  renders a quarter of a pixel at 22px and a clumsy slab at 86px. Weights are
  declared in *rendered* pixels and converted, which is why `constellation()`
  takes the display size.
- **Partial fill.** Stretching each figure fully to its bounding box evens out
  optical weight in a list, but snaps too many of them into axis-aligned Ls and
  brackets that read as geometry rather than sky. `fill` pulls most of the way
  and stops short.
