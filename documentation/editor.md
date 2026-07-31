# Fanticon Editor and Command Console

Fanticon has two application modes:

- **Editor mode** runs native host tools and is the default at launch.
- **Game mode** will run fantasy-console cartridges through the emulated machine.

The boot logo transitions to a command console in the selected mode. Editor
tools do not execute on the 6502; the prompt parses input and dispatches native
host actions. This keeps development tools responsive and unrestricted by the
fantasy machine while preserving Game mode for authentic cartridge execution.

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

The native console is exactly 40 columns by 25 rows. Each cell is rendered from
an embedded 8×8 character ROM, matching the 320×200 framebuffer without fractional
cell positioning. The ROM contains uppercase ASCII letters, numbers, and command
punctuation. Typed lowercase characters are normalized to uppercase.

The terminal writes character-ROM pixels into indexed video memory. It does not
ask the operating system to draw text, so appearance and layout are identical on
Windows, Linux, macOS, and WebAssembly. The final GPU presentation still supplies
CRT beam reconstruction, composite filtering, bloom, and scanline character.

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

Unknown commands produce a visible error and return to the prompt. Backspace
edits the current line, Enter submits it, and output scrolls when it reaches the
bottom row.

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
