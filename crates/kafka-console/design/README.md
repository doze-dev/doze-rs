# The console's design contract

These files are the **contract** the console pages are held to. They are copies of the canvas, not originals.

| Path | What it is | Who writes it |
|---|---|---|
| `boards/<section>/<id>.html` | The 96 board sources: a `<!--screen {json} -->` front-matter line, then the main-content markup in the kit's classes (`[[g:name 16]]` glyph tokens, `href="@id"` links, `data-op`, `data-ui`, `data-extra`, `data-scaffold`) | copied from the canvas by `tools/boards-refresh.ts`; never edited by hand |
| `kit/paper.css`, `kit/kafka.css` | The canvas kit's CSS, the source of the console's stylesheet and of the a11y lint's colour tokens | same |
| `boards.json` | The signature of every board (`tools/board-signature.ts`), deterministic | `bun tools/boards-snapshot.ts`; CI runs it with `--check` |
| `routes.json` | Screen id to route pattern (`{"topic-detail": "/topics/{topic}"}`) | generated from `routes.rs` by the Rust side |
| `allow.json` | The reviewed differences between pages and boards (`[{board, field, selector?, reason, expires?}]`) | by hand, each with a reason |

The boards are what the pages are compared with (`bun tools/board-diff.ts`, see `docs/port/console/gate.md`). They are not rendered in a browser by the gate.

## Refreshing from the canvas

The canvas lives outside the repository (the directory with `src/`, `kit/` and `shots/`). When the canvas changes:

```
bun tools/boards-refresh.ts <canvas-dir>        # copies src/<section>/*.html and kit/{paper,kafka}.css, checks every front matter id,
                                                # reports the ids added, removed and changed, and the count
bun tools/boards-snapshot.ts                    # regenerates boards.json
bun test tools/test                             # the tool tests, including identity for every board
bun tools/board-diff.ts                         # the built pages against the new boards
git add crates/kafka-console/design && git commit
```

`--dry-run` on the refresh lists what would change and writes nothing. A board that disappears from the canvas disappears here; if a page was built for it, `board-diff` no longer knows the id and the allow-list entries naming it become errors, which is the point.

`docs/` and `_docs/` in the canvas `src/` are direction boards, not screens; the refresh skips them and refuses a source without front matter.
