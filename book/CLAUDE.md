# Fanticon Developer's Guide — book build instructions

This folder contains the source for **Fanticon Developer's Guide.pdf**, a
retro-90s-styled hardware manual for the Fanticon fantasy console, rendered
from HTML/CSS via weasyprint. Everything needed to build or extend it lives
in this folder.

## Build

```
cd book/
python3 build.py
python3 -m weasyprint fanticon_full.html "Fanticon Developer's Guide.pdf"
```

`build.py` concatenates the HTML parts (in the exact order listed inside
it) into `fanticon_full.html`, wrapped in a shell `<head>` that links
`style.css`. weasyprint then renders that single file to PDF. There is no
other build step and no JS toolchain involved.

weasyprint renders take several seconds and can exceed a single 45-second
tool call. Background it and poll:

```
nohup python3 -m weasyprint fanticon_full.html "Fanticon Developer's Guide.pdf" > /tmp/weasy.log 2>&1 &
PID=$!
for i in $(seq 1 40); do kill -0 $PID 2>/dev/null || break; sleep 1; done
```

To visually QA a page after a build, render it to PNG with pypdfium2:

```python
import pypdfium2 as pdfium
doc = pdfium.PdfDocument("Fanticon Developer's Guide.pdf")
page = doc[79]  # 0-indexed
page.render(scale=2.0).to_pil().save("/tmp/check.png")
```
`get_textpage().get_text_range()` on a page lets you search for text to
find the right page number before rendering it.

## File layout

- `style.css` — the entire design system (colors, fonts, box/table/code
  styles, `@page` rules). See below.
- `build.py` — concatenation script. Edit the `parts` list here if you add
  or reorder chapters/appendices.
- `frontmatter.html` — cover, colophon, table of contents.
- `part1.html` … `part7_gamedev.html` — Parts I–VII. `part7_gamedev.html`
  is Part VII (game-dev patterns); despite the different filename it's a
  normal part in the build order.
- `appendixA.html` … `appendixEFG.html` — reference appendices (opcode
  table, memory map, etc).
- `assets/logo_transparent.png` — the only image asset actually referenced
  by the HTML (cover logo). Don't assume other files in `assets/` are used
  without grepping first.
- `fanticon_full.html` — generated output of `build.py`. Don't hand-edit;
  it gets overwritten on every build.
- `Fanticon Developer's Guide.pdf` — the final deliverable.

## Design system conventions (style.css)

- Divider/part-opener pages: `<div class="divider-page hue-X" id="partN">`
  where X is one of `cyan / purple / pink / mustard / green / slate / ink
  / coral`. Each part gets a distinct hue.
- Chapters: `<div class="chapter" id="chN">` containing a
  `<div class="chapter-kicker">CHAPTER N &middot; PART X</div>` and an
  `<h1 class="chapter-title">`. `.chapter { break-before: page; }` — every
  chapter starts on a fresh page automatically.
- Callout boxes: `<div class="box tip|warn|hw|try">` with a
  `<div class="box-title">LABEL</div>` inside. `tip` = green (best
  practice), `warn` = orange (pitfall), `hw` = cyan (hardware detail),
  `try` = purple (hands-on exercise).
- Code blocks: `<pre class="code">` with manual syntax-highlight spans:
  `.lbl` (labels), `.op` (instructions), `.dir` (directives), `.num`
  (numeric literals), `.cmt` (comments), `.str` (strings). Match this
  scheme in any new code sample rather than inventing new span classes.
- Tables: plain `<table>` for normal content tables; `class="reg-table"`
  for register-layout tables (cyan header) and `class="audio-table"` for
  audio register tables (magenta header). The dense reference tables in
  the appendices use `class="dense"` (or `.opcode-table`) and are wrapped
  in `<thead>/<tbody>` so a repeating header survives a page-break — every
  other table in the book uses flat `<tr>` children directly and should
  stay that way (see the row-striping note in style.css around line 410
  if you convert one to thead/tbody: the even/odd CSS selectors are split
  on purpose to keep striping identical either way).
- **Bit/attribute-field bytes must always be a table, never inline
  prose.** The established pattern is a small two-column sub-table right
  after the register/byte is introduced:
  ```html
  <table class="reg-table">
    <tr><th>Bit</th><th><code>REGISTER_NAME</code> field</th></tr>
    <tr><td>7</td><td>Enable</td></tr>
    <tr><td>6</td><td>...</td></tr>
    <tr><td>3–0</td><td>...</td></tr>
  </table>
  ```
  This has been retrofitted everywhere once already (see chapters 7–10,
  part4 audio registers, appendixB) after the user flagged inline bit
  descriptions as easy to miss. Don't reintroduce inline "bit 7 =
  enable, bits 6–5 = duty, ..." prose for any new register.
- Pagination: `p`, `li`, `.box`, `pre.code`, `table`, `tr`, `.figure` all
  have `break-inside: avoid` so normal content never splits mid-block
  across a page boundary. The two genuinely-too-long reference tables
  (`.dense table`, `.opcode-table`) are the deliberate exception — they're
  allowed to break between rows with a repeating `<thead>`. If you add
  another long reference table that legitimately needs to span pages,
  give it `.dense` (or add it to that CSS rule) and convert it to
  `<thead>/<tbody>`; otherwise leave tables in the default no-split mode.

## Voice and content rules

This is a hardware manual written for someone who already has Fanticon
installed — not a spec for implementers, not build-from-source docs.
Established constraints (all enforced by explicit user correction during
earlier drafting, don't reintroduce):

- No mention of the engine's implementation language, no references to
  tests/test suites, no "here's what the console doesn't support" framing.
- No hype qualifiers — never describe something as "real", "actual", or
  "actually" doing a thing. (Factual technical statements like "does not
  reload" are fine; it's the hype adjectives that are banned, not the
  underlying facts.)
- Register/bit-field bytes are always tables (see above), never inline.
- Code examples should be complete and runnable-looking (real label
  names, real EQU addresses, comments explaining the non-obvious lines),
  matching the existing chapter 7–10 / part6 examples in style.

## Ground truth for technical content

Before writing or changing any register address, bit layout, memory map
detail, or timing figure, verify it against the actual engine source
rather than trusting memory or prior book text:

- `/Users/lunaticedit/Projects/Fanticon/src/system.rs` — I/O register
  read/write match arms, bit masks (this is the most authoritative file
  for register behavior).
- `/Users/lunaticedit/Projects/Fanticon/src/video.rs` — palette format
  (`rgb332_to_rgba`), VRAM layout constants.
- `/Users/lunaticedit/Projects/Fanticon/src/audio.rs` — pulse/noise/master
  control byte semantics.
- `/Users/lunaticedit/Projects/Fanticon/src/machine.rs` — CPU clock,
  memory map, bank-switching window behavior.
- `/Users/lunaticedit/Projects/Fanticon/documentation/system-architecture.md`
  — human-readable cross-check, but treat the `.rs` source as the tiebreaker
  if it and this doc ever disagree.

Key facts already verified (safe to reuse, but re-check if the source has
since changed): NMOS 6502 @ 3.144 MHz, 60 Hz / 262 scanlines / 400
dots/line, 320×200 indexed video, 256-color RGB332 palette in 16 banks of
16. Memory map: `$0000–$7FFF` main RAM, `$8000–$BFFF` banked window
(16 KB), `$C000–$C0FF` I/O, `$C100–$FFFF` fixed ROM. Bank window selected
via `BANK_KIND` (`$C000`) / `BANK_NUMBER` (`$C001`); kinds are
CARTRIDGE_ROM=0, WORK_RAM=1, VIDEO_RAM=2 (3 banks), SAVE_RAM=3 (up to 4
banks). VRAM: TILE_PATTERNS at `$0000` (8 KB), TILE_MAP at `$2000`
(2048 B, 64×32), TILE_ATTRIBUTES at `$2800` (2048 B), SPRITE_TABLE at
`$3000` (256 B, 32×8-byte records), BITMAP at `$4000` (spans VRAM banks
1–2). Palette registers: `PALETTE_INDEX` = `$C01B`, `PALETTE_DATA` =
`$C01C` (auto-increments on both read and write, wraps at 255),
`BITMAP_PALETTE` = `$C01D`. Sprite limits: 32 records total, 8 drawn per
scanline.

## Other docs in this project

`/Users/lunaticedit/Projects/Fanticon/documentation/` holds the original
per-topic reference markdown (`6502.md`, `assembler.md`, `audio.md`,
`cartridge-format.md`, `cartridge-projects.md`, `editor.md`,
`graphics-editor.md`, `memory-map.md`, `music-editor.md`,
`system-architecture.md`, `system-details-checklist.md`, `video.md`).
Those are the raw source notes this book was written from — useful
background, but the book itself and this `book/` folder are the
canonical, polished deliverable.
