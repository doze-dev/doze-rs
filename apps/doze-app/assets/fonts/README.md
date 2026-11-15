# Fonts

Geist and Geist Mono, version 1.9.2, from the official release
(https://github.com/vercel/geist-font/releases/tag/v1.9.2, `geist-font-v1.9.2.zip`), licensed under the SIL Open
Font License 1.1 (`Geist-OFL.txt`, the release's `OFL.txt`; it covers both families and reserves no font name).

The files here are cut from the release's variable fonts, `Geist/variable/Geist[wght].ttf` and
`GeistMono/variable/GeistMono[wght].ttf`:

| file | weight |
|---|---|
| `Geist-400.ttf` … `Geist-750.ttf` | 400, 500, 600, 650, 700, 750 |
| `GeistMono-400.ttf` … `GeistMono-700.ttf` | 400, 500, 600, 700 |

Why static instances: GPUI on macOS loads a font from memory as one face at its default instance (a variable font
answers every weight with 400), so each weight `theme/ty.rs` names is its own file. Cut from the variable font, the
odd weights (Go's 650 headings and 750 wordmark) are exact rather than rounded to a neighbour.

Each file is subset to Basic Latin, Latin-1, general punctuation, arrows, the technical block (⏎ ⌫ ⌘ where the face
has them), box drawing and geometric shapes; glyphs reachable only through stylistic sets are dropped. A character the
faces lack (⌘, ✓, ✗, ⊘, ◐ and the like) falls back to the system font, as before.

To remake them (bun, no Python):

```sh
mkdir /tmp/geist-fonts && cd /tmp/geist-fonts
bun init -y && bun add subset-font@2.9.0 fontkit@3.0.0
cp <repo>/apps/doze-app/assets/fonts/make-fonts.ts .
bun make-fonts.ts 'Geist[wght].ttf' <repo>/apps/doze-app/assets/fonts Geist Geist 400,500,600,650,700,750
bun make-fonts.ts 'GeistMono[wght].ttf' <repo>/apps/doze-app/assets/fonts GeistMono GeistMono 400,500,600,700
```
