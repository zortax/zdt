#!/usr/bin/env python3
"""Writes the theme files from one table of palettes.

A theme is a hundred and forty custom properties, and almost all of them are the same nine or ten
colours arranged the same way. Writing that by hand once is fine; writing it eight times is how two
themes end up disagreeing about which token the status line reads.

So each theme is a palette here — the colours its authors published — and the arrangement is one
piece of code below it. Adding a theme is a dozen lines, and a change to how the interface uses a
colour is one line rather than eight.

Run it from anywhere:

    python3 assets/themes/generate.py

It rewrites every generated file and leaves the hand-written ones alone. Every value is from the
theme's own repository; where a theme publishes nothing for something this editor needs — Vesper
names no cursor colour — the choice is marked in the file it lands in.
"""

from __future__ import annotations

import pathlib
import textwrap
from dataclasses import dataclass, field

HERE = pathlib.Path(__file__).resolve().parent


@dataclass
class Palette:
    """One theme's colours, in the vocabulary its authors used."""

    # What the file says about itself.
    title: str
    note: str
    dark: bool

    # The grounds.
    bg: str
    fg: str
    sidebar: str
    popover: str
    muted: str  # a plane slightly off the ground: the current line, a hover
    accent: str  # a plane further off it: a selection band, a hovered row
    selection: str
    cursor: str
    comment: str
    gutter: str  # line numbers nobody is on
    subtle: str  # dimmer than the foreground, brighter than a comment

    # The hues. `orange` may repeat `yellow` where a palette has no separate one.
    red: str
    green: str
    yellow: str
    blue: str
    magenta: str
    cyan: str
    orange: str
    purple: str

    # The terminal's sixteen, when the theme publishes them. Normal then bright.
    ansi: list[str] = field(default_factory=list)

    # Borders are alpha over whatever is behind them, so they follow the surface rather than the
    # palette. Overridden only by a theme that publishes a real border colour.
    border: str | None = None
    sidebar_border: str | None = None

    def resolved_border(self) -> str:
        if self.border:
            return self.border
        return "oklch(1 0 0 / 8%)" if self.dark else "oklch(0 0 0 / 9%)"

    def resolved_input(self) -> str:
        return "oklch(1 0 0 / 12%)" if self.dark else "oklch(0 0 0 / 14%)"

    def resolved_sidebar_border(self) -> str:
        if self.sidebar_border:
            return self.sidebar_border
        return "oklch(1 0 0 / 10%)" if self.dark else "oklch(0 0 0 / 10%)"

    def resolved_ansi(self) -> list[str]:
        """Sixteen colours, made from the hues when the theme publishes none."""
        if self.ansi:
            return self.ansi
        return [
            self.accent, self.red, self.green, self.yellow,
            self.blue, self.magenta, self.cyan, self.fg,
            self.comment, self.red, self.green, self.orange,
            self.blue, self.magenta, self.cyan, self.fg,
        ]


def render(name: str, palette: Palette) -> str:
    """One theme file."""
    ansi = palette.resolved_ansi()
    surface = "dark" if palette.dark else "light"

    note = "\n".join(
        f" * {line}" for line in textwrap.wrap(palette.note, width=96)
    )

    return f"""/* {palette.title}, for a {surface} surface.
 *
{note}
 *
 * Written by `assets/themes/generate.py` — change the palette there, not the colours here.
 *
 * Two vocabularies live in this file. `--zui-*` are the component library's tokens, read by the
 * theme provider; everything else is this application's own, read straight from the cascade by
 * the editor, the terminal and the chrome. */

/* --- the component library ------------------------------------------------------------- */

--zui-color-background: {palette.bg};
--zui-color-foreground: {palette.fg};
--zui-color-card: {palette.bg};
--zui-color-card-foreground: {palette.fg};
--zui-color-popover: {palette.popover};
--zui-color-popover-foreground: {palette.fg};

--zui-color-primary: {palette.blue};
--zui-color-primary-foreground: {palette.bg};
--zui-color-secondary: {palette.accent};
--zui-color-secondary-foreground: {palette.fg};

--zui-color-muted: {palette.muted};
--zui-color-muted-foreground: {palette.comment};
--zui-color-accent: {palette.accent};
--zui-color-accent-foreground: {palette.fg};

--zui-color-destructive: {palette.red};
--zui-color-destructive-foreground: {palette.bg};
--zui-color-success: {palette.green};
--zui-color-success-foreground: {palette.bg};
--zui-color-warning: {palette.yellow};
--zui-color-warning-foreground: {palette.bg};
--zui-color-info: {palette.blue};
--zui-color-info-foreground: {palette.bg};

/* Borders are alpha over whatever is behind them, never a solid grey — a solid line is visible
 * as a line, and this interface wants planes told apart by tone. */
--zui-color-border: {palette.resolved_border()};
--zui-color-input: {palette.resolved_input()};
--zui-color-ring: {palette.blue};
--zui-color-scrim: oklch(0 0 0 / 55%);

--zui-color-sidebar: {palette.sidebar};
--zui-color-sidebar-foreground: {palette.subtle};
--zui-color-sidebar-primary: {palette.blue};
--zui-color-sidebar-primary-foreground: {palette.bg};
--zui-color-sidebar-accent: {palette.accent};
--zui-color-sidebar-accent-foreground: {palette.fg};
--zui-color-sidebar-border: {palette.resolved_sidebar_border()};
--zui-color-sidebar-ring: {palette.blue};

/* --- the application ------------------------------------------------------------------- */

--zdt-danger: {palette.red};
--zdt-warning: {palette.yellow};
--zdt-info: {palette.blue};
--zdt-hint: {palette.cyan};
--zdt-added: {palette.green};
--zdt-changed: {palette.yellow};
--zdt-removed: {palette.red};
--zdt-leap-label: {palette.orange};
--zdt-leap-label-fg: {palette.bg};
--zdt-match: {palette.yellow};

/* Mode colours, used by the status line block and nothing else. */
--zdt-mode-normal: {palette.blue};
--zdt-mode-insert: {palette.green};
--zdt-mode-visual: {palette.purple};
--zdt-mode-replace: {palette.red};
--zdt-mode-command: {palette.yellow};
--zdt-mode-terminal: {palette.cyan};

/* --- devicon tints ------------------------------------------------------------------------ */

--zdt-icon-rust: {palette.orange};
--zdt-icon-python: {palette.yellow};
--zdt-icon-ts: {palette.blue};
--zdt-icon-js: {palette.yellow};
--zdt-icon-go: {palette.cyan};
--zdt-icon-c: {palette.blue};
--zdt-icon-lua: {palette.blue};
--zdt-icon-shell: {palette.green};
--zdt-icon-html: {palette.orange};
--zdt-icon-css: {palette.blue};
--zdt-icon-config: {palette.yellow};
--zdt-icon-doc: {palette.fg};
--zdt-icon-image: {palette.purple};
--zdt-icon-git: {palette.orange};

/* --- the editor element ----------------------------------------------------------------- */

--editor-bg: {palette.bg};
--editor-fg: {palette.fg};
--editor-selection: {palette.selection};
--editor-cursor: {palette.cursor};
--editor-cursor-text: {palette.bg};
--editor-current-line: {palette.muted};
--editor-gutter-bg: {palette.bg};
--editor-gutter-fg: {palette.gutter};
--editor-gutter-current-fg: {palette.subtle};
--editor-scrollbar-thumb: {"oklch(1 0 0 / 10%)" if palette.dark else "oklch(0 0 0 / 14%)"};
--editor-scrollbar-track: transparent;
--editor-search: {palette.accent};
--editor-search-current: {palette.yellow};
--editor-search-current-text: {palette.bg};
--editor-indent-guide: {palette.accent};

/* --- syntax, one property per tree-sitter capture ---------------------------------------- */

--syntax-attribute: {palette.red};
--syntax-comment: {palette.comment};
--syntax-comment-doc: {palette.gutter};
--syntax-constant: {palette.orange};
--syntax-constant-builtin: {palette.orange};
--syntax-constructor: {palette.yellow};
--syntax-embedded: {palette.fg};
--syntax-escape: {palette.orange};
--syntax-function: {palette.blue};
--syntax-function-builtin: {palette.blue};
--syntax-function-macro: {palette.red};
--syntax-function-method: {palette.blue};
--syntax-keyword: {palette.magenta};
--syntax-label: {palette.magenta};
--syntax-module: {palette.cyan};
--syntax-number: {palette.orange};
--syntax-operator: {palette.cyan};
--syntax-property: {palette.subtle};
--syntax-punctuation: {palette.subtle};
--syntax-punctuation-bracket: {palette.subtle};
--syntax-punctuation-delimiter: {palette.comment};
--syntax-punctuation-special: {palette.orange};
--syntax-string: {palette.green};
--syntax-string-special: {palette.orange};
--syntax-tag: {palette.red};
--syntax-type: {palette.yellow};
--syntax-type-builtin: {palette.yellow};
--syntax-variable: {palette.fg};
--syntax-variable-builtin: {palette.red};
--syntax-variable-parameter: {palette.orange};

/* --- what git says ------------------------------------------------------------------------ */

--zdt-git-added: {palette.green};
--zdt-git-changed: {palette.blue};
--zdt-git-removed: {palette.red};
--zdt-git-untracked: {palette.subtle};
--zdt-git-conflict: {palette.orange};

/* The grounds a diff's rows sit on. Barely there: what carries the meaning is the mark and the
   colour of it, and a band strong enough to notice is a band that makes the code harder to read
   than the plain file it came from. */

--zdt-git-added-bg: color-mix(in oklch, {palette.green} 12%, transparent);
--zdt-git-removed-bg: color-mix(in oklch, {palette.red} 12%, transparent);

/* The graph's lanes. Eight, cycling: enough that two lines crossing are different colours, few
   enough that each stays distinct at two pixels wide. */

--zdt-git-lane-0: {palette.blue};
--zdt-git-lane-1: {palette.green};
--zdt-git-lane-2: {palette.magenta};
--zdt-git-lane-3: {palette.yellow};
--zdt-git-lane-4: {palette.cyan};
--zdt-git-lane-5: {palette.orange};
--zdt-git-lane-6: {palette.red};
--zdt-git-lane-7: {palette.subtle};

/* --- what the language servers say ------------------------------------------------------- */

--zdt-diagnostic-error: {palette.red};
--zdt-diagnostic-warning: {palette.yellow};
--zdt-diagnostic-information: {palette.blue};
--zdt-diagnostic-hint: {palette.cyan};

/* What the servers say at length: a hover, a completion's documentation. The block ground is the
   same plane a selection sits on, so a signature reads as inset rather than as a card. */

--zdt-doc-heading: {palette.fg};
--zdt-doc-rule: {palette.gutter};
--zdt-doc-code: {palette.green};
--zdt-doc-link: {palette.blue};
--zdt-doc-block: {palette.muted};

/* The kinds of thing a completion can offer. Four groups rather than twenty-five: what somebody
   reads off the glyph is "is this a function, a type, a value or a word", and a palette with a
   colour per protocol constant is a palette that says nothing. */

--zdt-completion-function: {palette.blue};
--zdt-completion-type: {palette.yellow};
--zdt-completion-value: {palette.orange};
--zdt-completion-keyword: {palette.magenta};
--zdt-completion-text: {palette.subtle};

/* Where a symbol is used, under the caret. Dimmer than a search hit, because it appears without
   being asked for and must not compete with the text it is marking. */

--zdt-highlight: {palette.accent};
--zdt-highlight-write: {palette.selection};

/* --- the terminal element ---------------------------------------------------------------- */

--terminal-background: {palette.bg};
--terminal-foreground: {palette.fg};
--terminal-cursor: {palette.cursor};
--terminal-cursor-text: {palette.bg};
--terminal-selection: {palette.selection};
--terminal-match: {palette.accent};
--terminal-match-current: {palette.yellow};

--terminal-black: {ansi[0]};
--terminal-red: {ansi[1]};
--terminal-green: {ansi[2]};
--terminal-yellow: {ansi[3]};
--terminal-blue: {ansi[4]};
--terminal-magenta: {ansi[5]};
--terminal-cyan: {ansi[6]};
--terminal-white: {ansi[7]};

--terminal-bright-black: {ansi[8]};
--terminal-bright-red: {ansi[9]};
--terminal-bright-green: {ansi[10]};
--terminal-bright-yellow: {ansi[11]};
--terminal-bright-blue: {ansi[12]};
--terminal-bright-magenta: {ansi[13]};
--terminal-bright-cyan: {ansi[14]};
--terminal-bright-white: {ansi[15]};
"""


# ---- The palettes ---------------------------------------------------------------------------
#
# Every value below is from the theme's own repository. Where this editor needs something a theme
# does not publish, the choice is noted beside it.

THEMES: dict[str, Palette] = {
    "vesper-dark": Palette(
        title="Vesper",
        note=(
            "Rauno Freiberg's, and the reason it looks unlike the rest: the whole palette is "
            "seven colours — a flat near-black, white text, a peach and a mint. There is no blue, "
            "no cyan and no magenta, so the tokens that name those hold the peach and the mint "
            "instead. Vesper publishes no cursor colour, no current-line tint and no terminal "
            "palette; the cursor is the peach, the current line is the tab tone, and the sixteen "
            "are built from what there is."
        ),
        dark=True,
        bg="#101010",
        fg="#ffffff",
        # Vesper uses one flat ground everywhere. The sidebar takes the tab tone so the panels are
        # told apart at all, which is this editor's arrangement rather than Vesper's.
        sidebar="#0c0c0c",
        popover="#161616",
        muted="#1c1c1c",
        accent="#232323",
        selection="#333333",
        cursor="#ffc799",
        comment="#575757",
        gutter="#505050",
        subtle="#a0a0a0",
        red="#ff8080",
        green="#99ffe4",
        yellow="#ffc799",
        blue="#a0a0a0",
        magenta="#ffcfa8",
        cyan="#99ffe4",
        orange="#ffc799",
        purple="#ffcfa8",
        ansi=[
            "#232323", "#ff8080", "#99ffe4", "#ffc799",
            "#a0a0a0", "#ffcfa8", "#99ffe4", "#a0a0a0",
            "#505050", "#ff8080", "#99ffe4", "#ffcfa8",
            "#65737e", "#ffcfa8", "#99ffe4", "#ffffff",
        ],
    ),
    "rose-pine-dark": Palette(
        title="Rosé Pine",
        note=(
            "The `main` variant. Rosé Pine names its own hues — love, gold, rose, pine, foam, "
            "iris — and the mapping here is the one its neovim port uses: pine for green, foam "
            "for blue, rose for cyan, iris for magenta. Bright and normal are the same colour "
            "throughout, which is the theme's own decision."
        ),
        dark=True,
        bg="#191724",
        fg="#e0def4",
        sidebar="#16141f",
        popover="#1f1d2e",
        muted="#21202e",
        accent="#26233a",
        selection="#403d52",
        cursor="#e0def4",
        comment="#908caa",
        gutter="#6e6a86",
        subtle="#908caa",
        red="#eb6f92",
        green="#31748f",
        yellow="#f6c177",
        blue="#9ccfd8",
        magenta="#c4a7e7",
        cyan="#ebbcba",
        orange="#f6c177",
        purple="#c4a7e7",
        ansi=[
            "#26233a", "#eb6f92", "#31748f", "#f6c177",
            "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
            "#908caa", "#eb6f92", "#31748f", "#f6c177",
            "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
        ],
    ),
    "rose-pine-light": Palette(
        title="Rosé Pine Dawn",
        note=(
            "The `dawn` variant. The text is `#464261`, which is the current canonical value — "
            "the alacritty distribution still ships the older `#575279`."
        ),
        dark=False,
        bg="#faf4ed",
        fg="#464261",
        sidebar="#f8f0e7",
        popover="#fffaf3",
        muted="#f4ede8",
        accent="#dfdad9",
        selection="#dfdad9",
        cursor="#464261",
        comment="#797593",
        gutter="#9893a5",
        subtle="#797593",
        red="#b4637a",
        green="#286983",
        yellow="#ea9d34",
        blue="#56949f",
        magenta="#907aa9",
        cyan="#d7827e",
        orange="#ea9d34",
        purple="#907aa9",
        ansi=[
            "#f2e9e1", "#b4637a", "#286983", "#ea9d34",
            "#56949f", "#907aa9", "#d7827e", "#464261",
            "#797593", "#b4637a", "#286983", "#ea9d34",
            "#56949f", "#907aa9", "#d7827e", "#464261",
        ],
    ),
    "catppuccin-dark": Palette(
        title="Catppuccin Mocha",
        note=(
            "The Mocha flavour. The sixteen terminal colours are Catppuccin's own `ansiColors` "
            "table, which is separately tuned rather than the accents repeated."
        ),
        dark=True,
        bg="#1e1e2e",
        fg="#cdd6f4",
        sidebar="#181825",
        popover="#181825",
        muted="#2a2b3c",
        accent="#313244",
        selection="#45475a",
        cursor="#f5e0dc",
        comment="#9399b2",
        gutter="#6c7086",
        subtle="#a6adc8",
        red="#f38ba8",
        green="#a6e3a1",
        yellow="#f9e2af",
        blue="#89b4fa",
        magenta="#f5c2e7",
        cyan="#94e2d5",
        orange="#fab387",
        purple="#cba6f7",
        ansi=[
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
            "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
            "#585b70", "#f37799", "#89d88b", "#ebd391",
            "#74a8fc", "#f2aede", "#6bd7ca", "#bac2de",
        ],
    ),
    "catppuccin-light": Palette(
        title="Catppuccin Latte",
        note="The Latte flavour, with Catppuccin's own `ansiColors` table for the terminal.",
        dark=False,
        bg="#eff1f5",
        fg="#4c4f69",
        sidebar="#e6e9ef",
        popover="#e6e9ef",
        muted="#e9ebf1",
        accent="#ccd0da",
        selection="#bcc0cc",
        cursor="#dc8a78",
        comment="#7c7f93",
        gutter="#9ca0b0",
        subtle="#6c6f85",
        red="#d20f39",
        green="#40a02b",
        yellow="#df8e1d",
        blue="#1e66f5",
        magenta="#ea76cb",
        cyan="#179299",
        orange="#fe640b",
        purple="#8839ef",
        ansi=[
            "#5c5f77", "#d20f39", "#40a02b", "#df8e1d",
            "#1e66f5", "#ea76cb", "#179299", "#acb0be",
            "#6c6f85", "#de293e", "#49af3d", "#eea02d",
            "#456eff", "#fe85d8", "#2d9fa8", "#bcc0cc",
        ],
    ),
    "tokyonight-dark": Palette(
        title="Tokyo Night",
        note=(
            "The `night` variant. The bright terminal row is folke's own generated output rather "
            "than the normal row repeated."
        ),
        dark=True,
        bg="#1a1b26",
        fg="#c0caf5",
        sidebar="#16161e",
        popover="#16161e",
        muted="#292e42",
        accent="#283457",
        selection="#283457",
        cursor="#c0caf5",
        comment="#565f89",
        gutter="#3b4261",
        subtle="#a9b1d6",
        red="#f7768e",
        green="#9ece6a",
        yellow="#e0af68",
        blue="#7aa2f7",
        magenta="#bb9af7",
        cyan="#7dcfff",
        orange="#ff9e64",
        purple="#9d7cd8",
        ansi=[
            "#15161e", "#f7768e", "#9ece6a", "#e0af68",
            "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
            "#414868", "#ff899d", "#9fe044", "#faba4a",
            "#8db0ff", "#c7a9ff", "#a4daff", "#c0caf5",
        ],
    ),
    "gruvbox-dark": Palette(
        title="Gruvbox",
        note=(
            "The dark medium contrast. Gruvbox publishes no cursor colour — it reverses the text "
            "— so the cursor here is the foreground, which is what a block cursor comes to."
        ),
        dark=True,
        bg="#282828",
        fg="#ebdbb2",
        sidebar="#1d2021",
        popover="#32302f",
        muted="#32302f",
        accent="#3c3836",
        selection="#665c54",
        cursor="#ebdbb2",
        comment="#928374",
        gutter="#7c6f64",
        subtle="#bdae93",
        red="#fb4934",
        green="#b8bb26",
        yellow="#fabd2f",
        blue="#83a598",
        magenta="#d3869b",
        cyan="#8ec07c",
        orange="#fe8019",
        purple="#d3869b",
        ansi=[
            "#282828", "#cc241d", "#98971a", "#d79921",
            "#458588", "#b16286", "#689d6a", "#a89984",
            "#928374", "#fb4934", "#b8bb26", "#fabd2f",
            "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
        ],
    ),
    "gruvbox-light": Palette(
        title="Gruvbox Light",
        note="The light medium contrast, with the faded accent row its authors pair with it.",
        dark=False,
        bg="#fbf1c7",
        fg="#3c3836",
        sidebar="#f2e5bc",
        popover="#f2e5bc",
        muted="#ebdbb2",
        accent="#ebdbb2",
        selection="#bdae93",
        cursor="#3c3836",
        comment="#928374",
        gutter="#a89984",
        subtle="#665c54",
        red="#9d0006",
        green="#79740e",
        yellow="#b57614",
        blue="#076678",
        magenta="#8f3f71",
        cyan="#427b58",
        orange="#af3a03",
        purple="#8f3f71",
        ansi=[
            "#fbf1c7", "#cc241d", "#98971a", "#d79921",
            "#458588", "#b16286", "#689d6a", "#7c6f64",
            "#928374", "#9d0006", "#79740e", "#b57614",
            "#076678", "#8f3f71", "#427b58", "#3c3836",
        ],
    ),
}


def main() -> None:
    for name, palette in THEMES.items():
        path = HERE / f"{name}.css"
        path.write_text(render(name, palette), encoding="utf-8")
        print(f"wrote {path.name}")


if __name__ == "__main__":
    main()
