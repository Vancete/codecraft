# CodeCraft

A small, fast-starting code editor written in Rust with [egui](https://github.com/emilk/egui). It is a sibling of [gitcraft](https://github.com/Vancete/gitcraft) and shares its look: the same dark theme, tab strip, menus and Lucide icons.

## Features

- **VS Code-like layout**: an activity bar on the left (Explorer, Search, and at the bottom Terminal and Settings), a file tree of the open folder, editor tabs along the top and a status bar. Settings opens as a tab of its own.
- **Find in the file** (Ctrl+F, Ctrl+H to replace): VS Code's find bar over the editor, with match case, whole word and regular expression (Alt+C, Alt+W, Alt+R), "3 of 12", Enter / Shift+Enter (or F3 / Shift+F3) to move between matches, and replace one or all. Regular expression replacements can use `$1`.
- **Search in the folder** (Ctrl+Shift+F, Ctrl+Shift+H to replace): the side panel's Search view, with the same options, files to include and exclude (globs such as `*.rs, src/**`), and the matches grouped by file; a click opens the file at the match. The folder is searched in the background, skipping what `.gitignore` lists. Replace All asks first, then changes the files (open ones in the editor).
- **Terminal** (Ctrl+`): gitcraft's built-in terminal in a panel under the editor, one tab per shell, started in the open folder. The shell is chosen in the settings.
- **Editor**: a monospace editor with line numbers, a highlighted cursor line and syntax colouring ([syntect](https://github.com/trishume/syntect), re-coloured incrementally while you type). Files with CRLF line endings keep them when saved.
- **Tabs**: one per open file, with its file-type icon. A dot marks unsaved changes, and CodeCraft asks before it closes a tab or the window with unsaved work.
- **VS Code themes**: with *Settings → Use VS Code's colour theme and file icons* (on by default), CodeCraft reads VS Code's `settings.json` and takes:
  - the colour theme (`workbench.colorTheme`) for the editor's syntax colours and for the window's colours;
  - the file icon theme (`workbench.iconTheme`, Seti when it isn't set). Both font-based themes (Seti ships a WOFF font) and SVG/PNG themes (Material Icon Theme and most others) work. A theme whose font is WOFF2 falls back to the built-in icons.

  Without VS Code, or with the option off, CodeCraft uses gitcraft's Dark, Medium Gray or Light theme (the palette button) and its own per-file-type icons. VS Code, VS Code Insiders and VSCodium are looked for; the themes are read again when you come back to the window after changing them in VS Code.
- **Session**: the open folder, the open files, the expanded folders and the settings are restored on the next start.

## Usage

```bash
cargo run --release -- [folder-or-file]
```

| Shortcut | Action |
|---|---|
| Ctrl+O | Open a folder |
| Ctrl+F / Ctrl+H | Find / replace in the file |
| Enter, F3 / Shift+Enter, Shift+F3 | Next / previous match |
| Ctrl+Shift+F / Ctrl+Shift+H | Search / replace in the folder |
| Ctrl+Shift+E | Explorer |
| Ctrl+` (Ctrl+Ñ on a Spanish keyboard) | Show or hide the terminal |
| Ctrl+Shift+` | New terminal |
| Ctrl+S | Save |
| Ctrl+W | Close the tab |
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+B | Show or hide the side panel |
| Ctrl+, | Settings |

## Development

```bash
cargo clippy --all-targets
cargo test
```

`examples/ui_shot.rs` takes headless screenshots driven by a small script of clicks and keys (see the comment at its top), for checking the UI without a window:

```bash
cargo run --release --example ui_shot -- . shots/ui open:src/app.rs ckey:F type:self shot
```

`examples/make_icon.rs` renders `assets/app-icon/codecraft.svg` into the PNG and ICO files.

## Licence

MIT or Apache-2.0, at your option. Bundled fonts and icons: see [ATTRIBUTION.md](ATTRIBUTION.md).
