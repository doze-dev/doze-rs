# Patches to gpui-pre-apple 0.3.12

This is `gpui-pre-apple` 0.3.12 from crates.io (Zed's `gpui_apple`, the gpui-pre snapshot of zed@d2a4fb6), vendored,
with the change below, marked `PATCH` in the source. Only the desktop app (`apps/doze-app`) links it; the root
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

## 2. Lints

None changed: `vendor/` is excluded from the workspace, so its own `[lints]` table applies to it alone.
