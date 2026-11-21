# Patches to gpui-pre 0.3.12 and gpui-pre-macos 0.3.12

These are `gpui-pre` and `gpui-pre-macos` 0.3.12 from crates.io (Zed's `gpui` and `gpui_macos`, the gpui-pre snapshot
of zed@d2a4fb6), vendored with the changes below. Each change is marked `PATCH` in the source. The vendored copies
leave out upstream's `examples/`, `benches/` and `tests/`, and the manifest's `[[example]]` and `[[test]]` targets.
Only the desktop app (`apps/doze-app`) links them. The root `Cargo.toml` points `[patch.crates-io]` here (and at
`vendor/gpui-pre-macos`). `vendor/gpui-pre-apple` carries the same manifest change (its PATCHES.md §5).

## 1. The image crate with PNG only (`Cargo.toml` in all three crates, `src/platform.rs`)

Upstream depends on `image` with every format it can decode: bmp, dds, exr, ff, gif, hdr, ico, jpeg, png, pnm, qoi,
tga, tiff and webp, plus rayon. The list is hard-coded in the `[dependencies.image]` of gpui-pre, gpui-pre-macos and
gpui-pre-apple, not behind a feature, and cargo features only add, so the app cannot turn any of them off. The patch
sets `features = ["png"]` in all three.

What the app decodes:
- the two tray icons (`apps/doze-app/assets/tray/*.png`), which AppKit decodes (`NSImage`, doze-os `ui`), not `image`;
- SVG everywhere else: the icons and the brand mark. SVG goes through resvg and usvg, not through `image`.

Nothing in the app loads a JPEG, GIF, WebP, TIFF or any other raster format.

Code changes: `decode_gif_image` and `decode_webp_image` (`src/platform.rs`) used `image::codecs::{gif, webp}`, which
are not compiled without those features. They now return an error ("not built into this gpui"), so the image cache
shows the same failure it shows for any undecodable image. `decode_animated_frames` is kept for an upgrade to compare
against, with `#[allow(dead_code)]`. The unused `Rgba` import is gone.

**Measured** (darwin/arm64, `release-app`, 2026-11-15): 15,257,328 B to **14,526,560 B (−730,768 B)**, 650 to 634
package lines in CI's count. The crates that leave the app: exr, tiff, qoi, half, lebe, fax, bit_field, zune-inflate and
zerocopy-derive. gif, image-webp and zune-jpeg stay, because resvg uses them for images embedded in an SVG.

**Checked** (2026-11-15): I rendered `doze-app shot --fixture larder` with this build and with the build before the patch, in dark
and light, for library, tiles, map, palette, about and instance. The icons (home, bell, copy, the density lines) and
the bat mark (`brand/doze.svg`, through `img()`: `guess_format` refuses it and it goes to the SVG renderer) draw the
same in both builds. About is byte-identical. The other screens differ only in text the fixture's clock moves and in app-finish's
last commit, which the comparison build predates.

**Upgrading gpui-pre**: re-apply the manifest change to the three crates, and check that `decode_gif_image` and
`decode_webp_image` are still the only users of the dropped codecs (`rg 'image::codecs'`).

## 2. Lints

None changed: `vendor/` is outside the workspace.
