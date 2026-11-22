# Patches to gpui-pre-apple 0.3.12

This is `gpui-pre-apple` 0.3.12 from crates.io (Zed's `gpui_apple`, the gpui-pre snapshot of zed@d2a4fb6), vendored,
with the changes below, marked `PATCH` in the source. Only the desktop app (`apps/doze-app`) links it; the root
`Cargo.toml` points `[patch.crates-io]` here. The GPUI spike (`~/Code/doze-dev/doze-gpui-spike`) carried the same
change; its other edits there (`SPIKE_*` environment knobs for measuring) are not kept.

## 1. No MSAA for paths (`src/metal_renderer.rs`, `PATH_SAMPLE_COUNT`)

Upstream rasterises paths (the Map's curves and arrowheads, every `PathBuilder` shape) into a 4-sample intermediate
texture the size of the window, kept for as long as anything draws a path. One sample drops that texture's extra
samples.

Measured with `doze-app bench --fixture larder` (release, M-series Mac, 1280 × 800 window, macOS footprint as
Activity Monitor reports it, 2026-11-13):

| | 10 s after the window opened | 25 s |
|---|---|---|
| upstream (4 samples) | 87 MB | 79 MB |
| patched (1 sample) | 82 MB | 74 MB |

The app's budget for a window open and idle is 85 MB (docs/port/app/PLAN.md §6); upstream is over it at 10 s. The
texture grows with the window, so a full-screen window saves more.

What it costs: path edges are not multisampled. On a 2× display the Map's curves and heads still read cleanly (the
CI app job's `map-larder-*` shots); on a 1× display curves may show slight stair-stepping. Revisit when gpui-pre is
upgraded, or if the owner prefers smoother curves on 1× displays over the 5 MB.

## 2. No font smoothing for text (`src/text_system.rs`: `glyph_dilation_for_color`, `rasterize_glyph`)

Upstream draws each glyph with Core Graphics font smoothing. For light text it turns smoothing on and picks a
"dilation" level from the text colour's luminance (up to 4 for near-white); for dark text it leaves the bitmap
context's default, which smooths too. Smoothing thickens every stem. The Go app's page sets
`-webkit-font-smoothing: antialiased`, which WebKit draws as plain grayscale coverage with no smoothing, so the same
Inter and JetBrains Mono read visibly heavier in the Rust app: most in dark themes, mono most of all.

Patched: `glyph_dilation_for_color` is always 0, and the rasteriser turns smoothing off (`set_allows_font_smoothing
(false)`, `set_should_smooth_fonts(false)`). Upstream's dilation is kept beside it as
`upstream_glyph_dilation_for_color`, for an upgrade to compare against.

Measured on the same words from both apps at 2× (`doze-app shot` against Playwright WebKit on the Go page, shop
fixture, 2026-11-13): total ink against Go's went from 1.12–1.45 to 1.00 ± 0.01 on body, labels, numbers, buttons,
key chips, captions and mono, in dark and light themes. Nothing else changes: memory, binary and frame time are the
same (one branch fewer per glyph).

## 3. A count of frames drawn (`src/metal_renderer.rs`: `frames_drawn`)

`MetalRenderer::draw` adds one to a process-wide counter for each frame it commits to a window's drawable (offscreen
captures are not counted), and says `frame N` at trace level (target `gpui_apple::frames`; `DOZE_APP_LOG=trace`).
`gpui_apple::metal_renderer::frames_drawn()` reads it. The app's `bench` reports the frames an idle window drew and
fails `--check` when there are any: on this Mac the GPU driver holds about 150 MB for as long as a window keeps
drawing, even once a second (docs/port/weight.md, "the idle window"), so "draws nothing while idle" is the property
that keeps an open window in its budget. One relaxed atomic add per frame; nothing else changes.

## 4. Lints

None changed: `vendor/` is excluded from the workspace, so its own `[lints]` table applies to it alone.

## 5. The image crate with PNG only (`Cargo.toml`)

The same change as `vendor/gpui-pre/PATCHES.md` §1: `[dependencies.image]` lists `png` only. gpui-pre-apple has no code
that uses another format.

## 6. The path texture's pages go back between frames (`src/metal_renderer.rs`: `path_frames`, `render_frame`)

Upstream keeps the window-sized path intermediate texture (2560 × 1602 BGRA on a 1280 × 800 window at 2×, 16 MB,
private storage) resident from the first frame that draws a path for as long as the window lives. A page that draws
paths at all (a service's page: its meters' curves and its countdown ring) held those 16 MB while it sat idle, so an
idle service page read 98 MB against the app's 85 MB line.

Patched: each frame sets the texture `NonVolatile` before it is used (it is cleared before paths draw into it, so no
contents carry over); the frame's completion handler sets it `Empty` when no newer frame has been encoded since
(`path_frames`, a counter under a mutex the next frame also takes, so a frame never starts on a texture being
emptied). The texture object stays; only its pages go back, and the next frame takes them again. While frames come
back to back (in use, scrolling) a newer frame is always encoded before the last one completes, so nothing changes
there; a window drawing a frame now and then holds no 16 MB in between.

Measured (release-app, `doze-app bench --screen service --select api --on-top` over a live 14-service stack,
2026-11-21): the idle service page 98 MB settled before, 84 MB after (Owned physical footprint (unmapped) (graphics):
22 MB → 6 MB).
