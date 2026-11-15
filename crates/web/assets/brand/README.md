# Artwork

Ladderback's icons, favicons, logos and social preview. No rights are reserved: the artwork is dedicated to the public domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).

The mark is a ladder-back chair's back: two posts and three slats, white on the light theme's accent (`--accent`, `#1d4ed8`).

| File | Use |
|---|---|
| `icon.svg` | The master icon, a rounded square. The web UI's header draws its shapes inline beside the name (`BrandMark` in `crates/web/src/main.rs`), so a change to them is made there too |
| `icon-maskable.svg` | Full-bleed, with the mark inside the maskable safe zone, for Android's adaptive icons |
| `favicon.svg`, `favicon-16.png`, `favicon-32.png`, `favicon.ico` | Browser tabs; the SVG has heavier strokes, so it reads at 16 px |
| `apple-touch-icon.png` (180), `icon-192.png`, `icon-512.png`, `icon-maskable-512.png` | Home-screen and app icons (#394) |
| `logo.svg`, `logo-dark.svg` | The mark and the name, on light and dark backgrounds |
| `social-preview.svg`, `social-preview.png` (1280 × 640) | The repository's social preview |

The logos and social preview set the name in the system's sans-serif font, so the SVGs look slightly different from one machine to another; the PNG was rendered on macOS.

## Rendering the PNGs again

The SVGs are the source. After changing one, render its PNGs with [resvg](https://github.com/linebender/resvg) and the `.ico` with ImageMagick, at the sizes the table gives:

```sh
resvg -w 32 -h 32 favicon.svg favicon-32.png
resvg -w 512 -h 512 icon.svg icon-512.png
resvg -w 48 -h 48 favicon.svg /tmp/favicon-48.png
magick favicon-16.png favicon-32.png /tmp/favicon-48.png favicon.ico
```

Neither tool is needed to build Ladderback: the PNGs are committed.

It was drawn with an AI assistant's help, as SVG by hand.
