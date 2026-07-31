# Fanticon Editor and Command Console

Fanticon has two application modes:

- **Editor mode** runs native host tools and is the default at launch.
- **Game mode** will run fantasy-console cartridges through the emulated machine.

The boot logo transitions to a command console in the selected mode. Editor
tools do not execute on the 6502; the prompt parses input and dispatches native
host actions. This keeps development tools responsive and unrestricted by the
fantasy machine while preserving Game mode for authentic cartridge execution.

On native builds, `/` maps to the current user's operating-system Documents
directory under `Fanticon` (for example, `Documents/Fanticon`). Fanticon creates
that directory when needed and never permits console paths to escape it. The
prompt displays the current virtual path, beginning with `/>`. Browser builds
use an in-memory virtual root because web pages cannot directly mount the host's
Documents directory.

## Launching

Ordinary launches enter Editor mode:

```sh
cargo run --release
```

Use `--game` to start at the Game mode prompt:

```sh
cargo run --release -- --game
```

Inside the console, `EDITOR` and `GAME` switch modes. F1 selects Editor mode and
F2 selects Game mode. Mode changes rebuild the screen and prompt without
restarting the window or GPU renderer.

## Character display

Editor mode is 640×400 and its native console is exactly 80 columns by 50 rows.
Game mode remains the fantasy hardware's 320×200 output and uses a 40×25 prompt
until a cartridge takes ownership of the display. Each cell is rendered from an
embedded 8×8 character ROM without fractional positioning. The ROM contains
uppercase ASCII letters, numbers, and command punctuation. Typed lowercase
characters are normalized to uppercase.

The terminal writes character-ROM pixels into indexed video memory. It does not
ask the operating system to draw text, so appearance and layout are identical on
Windows, Linux, macOS, and WebAssembly. The final GPU presentation still supplies
CRT beam reconstruction, composite filtering, bloom, and scanline character.
Within each 8-pixel character row, the top retains the exact palette color and
the presentation darkens gradually by 20% toward the bottom. This restrained
phosphor-density gradient applies only to native Editor mode, not game output.

The ROM also reserves native UI glyphs for connected window borders, selection
arrows, build progress, success, and error symbols. Editor menus and dialogs clear
their backing cells before drawing these bordered windows, keeping source text and
syntax colors from bleeding through the overlay.

## Commands

| Command | Effect |
| --- | --- |
| `HELP` | List current commands |
| `CLS` or `CLEAR` | Clear the terminal |
| `MODE` | Print the active application mode |
| `EDITOR` | Enter native Editor mode |
| `GAME` | Enter Game mode |
| `ECHO text` | Print text |
| `VERSION` | Print the Fanticon host version |
| `COLOR [BG FG]` | Show or set the shared UI background and foreground palette indexes |
| `EDIT [file]` | Open the text editor, optionally loading an 8.3 text file |
| `CD [path]` | Change directory, or print the current directory with no path |
| `MKDIR path` | Create one directory |
| `RMDIR path` | Remove an empty directory |
| `DIR [path]` or `LS [path]` | List a directory |
| `BUILD source [output]` or `ASM source [output]` | Assemble ASM/INC source to a raw BIN file |
| `DUMP file [offset [length]]` | Display file bytes as hexadecimal and printable ASCII |

Console paths are lowercase and accept `/` or `\\` separators. Every file and
directory name uses 8.3 format: a 1–8 character name with an optional dot and
1–3 character extension. Names may contain ASCII letters, digits, `_`, and `-`;
spaces and other characters are rejected. Lookup on the native filesystem is
case-insensitive, so an existing mixed-case directory such as `MyGame` is
treated as `mygame`. New names are created lowercase. Existing host entries that
do not fit 8.3 are left untouched and hidden from `DIR` and `LS`. Absolute paths
begin at Fanticon's `/`; `..` cannot move above that root.

Unknown commands produce a visible error and return to the prompt. Backspace
edits the current line, Enter submits it, and output scrolls when it reaches the
bottom row.

`DUMP` uses an eight-byte hex-editor layout sized to the 40-column display.
Addresses, byte values, and printable ASCII use distinct lavender, peach, and
green colors. Zero bytes and their ASCII placeholders use a darker muted color,
making zero-filled areas recede visually. Address fields omit unnecessary leading zeroes and right-align to
the widest address in the displayed range, keeping every byte and ASCII column
aligned. It shows 128 bytes by default; pass an offset and optional length to
inspect another range. Numeric arguments accept decimal, `$` hexadecimal, or
`0x` hexadecimal:

```text
DUMP GAME.BIN
DUMP GAME.BIN $80
DUMP GAME.BIN 0x100 64
```

The console and text editor default to white (`255`) on black (`0`). Use
`COLOR BG FG` to select any two indexes from the 256-color RGB332 palette; for
example, `COLOR 0 255` restores the default. `COLOR` with no arguments reports
the current pair. Decimal, `$`-prefixed hexadecimal, and `0x`-prefixed
hexadecimal indexes are accepted. The setting immediately applies to both the
console and editor, including inverse-color menus and selections.

## Text editor

Run `EDIT` for a new document or `EDIT NOTES.TXT` to load a file from the
current console directory. The editor uses Fanticon's own character display and
text-mode dialogs; it does not open native operating-system controls.

The menu bar contains **File**, **Edit**, and **Build** menus. Press F10 or Alt+F for File,
Alt+E for Edit, Alt+B for Build, use the arrow keys to navigate, Enter to choose an item, and
Escape to close a menu. While a menu is open, its displayed single-letter
hotkeys activate items immediately. File uses N/O/S/A/X for New, Open, Save,
Save As, and Exit. Edit uses U/T/C/P/A for Undo, Cut, Copy, Paste, and Select
All. Build uses B/N/P for Assemble, Next Error, and Previous Error.

Editing supports the arrow, Home, End, Page Up, Page Down, Backspace, Delete,
Enter, and Tab keys. Hold Shift while moving to select text. The familiar
shortcuts Ctrl+A/C/X/V/Z are supported using an editor-local clipboard. Ctrl+N
creates a document, Ctrl+O opens the filename dialog, and Ctrl+S saves. F2 is a
Save shortcut and F3 opens a file. Escape dismisses an open menu or dialog but
does nothing in the document surface; exit explicitly through File > Exit.

On macOS, Option+F and Option+E open the File and Edit menus even though Option
changes the character generated by those keys. Command+A/C/X/V/Z/N/O/S are
accepted as the native equivalents of the Control shortcuts. Ctrl/Command+B or
F5 assembles the current ASM/INC buffer. F4 selects the next build diagnostic and
Shift+F4 selects the previous one.

Open and Save As use an on-screen filename dialog and the same sandboxed 8.3
filesystem as the console. Relative names begin in the console's current
directory; paths may use `/` or `\\`. The status bar displays the filename,
unsaved-change marker, line, and column.

### 6502 assembly mode

Opening or saving a file with an `.ASM` or `.INC` extension automatically enables
6502 assembly mode. Other extensions remain ordinary unformatted text. Assembly
source is arranged in a Merlin-inspired four-column layout when loaded or saved:

| Field | Display column |
| --- | ---: |
| Label | 1 |
| Opcode or directive | 10 |
| Operand | 16 |
| Comment | 27 |

The formatter recognizes the complete standard NMOS 6502 instruction set,
Fanticon's supported undocumented mnemonics, and common Merlin directives.
Semicolons begin comments except inside single- or double-quoted strings; a line
beginning with `*` or `;` is a full-line comment.

Assembly highlighting uses exact Catppuccin Mocha accents over a true-black
background: lavender labels, blue instructions, mauve directives, peach numeric
literals, muted overlay comments, green strings, and Catppuccin Text for other
source. ASM/INC mode reserves palette indexes 240–246 for these colors and
overrides the ordinary `COLOR` pair while that source is displayed. Selection
uses inverse Catppuccin Text and black. The saved source remains ordinary
case-preserving text with spaces; no Merlin high-ASCII file encoding is
introduced.

In assembly mode, Tab advances to the next label/opcode/operand/comment field.
Pressing Enter at the end of a line formats that line immediately. A line is also
formatted when ordinary cursor navigation leaves it; Shift-selection deliberately
suppresses this behavior so selecting source never rewrites it. Loading and saving
normalize every assembly line to the four-column layout.

Assembly produces a raw `.BIN` beside the source by default. The editor shows an
assembling dialog followed by a success dialog with the output, origin, and size,
or an error dialog with the current diagnostic. Successful results are shown as
white text on blue; build errors are white on red. Errors move the cursor to the
source location, tint affected lines Catppuccin red, and remain navigable until
the document changes. The complete syntax, directive, macro, and output reference
is in [Fanticon 6502 Macro Assembler](assembler.md).

## Extending the prompt

Command parsing lives in `Terminal::execute`. Commands that only print text can
write directly to the terminal. Commands that affect the host return a
`TerminalAction`; the app applies that action outside the parser. Mode switching
already follows this route.

Future editor launches should add action variants rather than creating windows,
files, or GPU resources inside the terminal parser. This keeps command handling
testable and gives the application one place to manage tool lifetimes. Likely
future actions include opening source, sprite, map, sound, palette, and cartridge
project editors.

Game and Editor mode currently share the command-console surface. Once cartridge
loading exists, Game mode can hand display ownership to the emulated video device
while Editor mode continues using native terminal and tool rendering.
