#!/usr/bin/env python3
"""Writes the theme files from one table of palettes.

A theme is a hundred and forty custom properties, and almost all of them are the same nine or ten
colours arranged the same way. Writing that by hand once is fine; writing it fifty times is how two
themes end up disagreeing about which token the status line reads.

So each theme is a palette here — the colours its authors published — and the arrangement is one
piece of code below it. Adding a theme is a dozen lines, and a change to how the interface uses a
colour is one line rather than fifty.

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
    "ayu-dark": Palette(
        title="Ayu Dark",
        note=(
            "Read from Ayu's own VS Code build. Ayu publishes two grounds and puts the editor on "
            "the lighter one, so the file tree sits below the text rather than above it. Its "
            "selection, its line numbers and its dimmed text are published with an alpha; the "
            "values here are those colours flattened over what they lie on, because a token this "
            "editor hands to the terminal cannot be half transparent. Ayu colours keywords orange "
            "and functions yellow, and this arrangement gives keywords the purple it uses for "
            "constants."
        ),
        dark=True,
        bg="#10141c",
        fg="#bfbdb6",
        sidebar="#0d1017",
        popover="#0f131a",
        muted="#161a24",
        accent="#1b1f29",
        selection="#193155",
        cursor="#e6b450",
        comment="#5a6673",
        gutter="#404758",
        subtle="#8b8b88",
        red="#f07178",
        green="#aad94c",
        yellow="#ffb454",
        blue="#59c2ff",
        magenta="#d2a6ff",
        cyan="#95e6cb",
        orange="#ff8f40",
        purple="#d2a6ff",
        ansi=[
            "#1b1f29", "#f06b73", "#70bf56", "#fdb04c",
            "#4fbfff", "#d0a1ff", "#93e2c8", "#c7c7c7",
            "#686868", "#f07178", "#aad94c", "#ffb454",
            "#59c2ff", "#d2a6ff", "#95e6cb", "#ffffff",
        ],
    ),
    "ayu-light": Palette(
        title="Ayu Light",
        note=(
            "The light surface of the same build, with the same flattening of the colours Ayu "
            "publishes with an alpha."
        ),
        dark=False,
        bg="#fcfcfc",
        fg="#5c6166",
        sidebar="#f8f9fa",
        popover="#ffffff",
        muted="#f0f1f3",
        accent="#eaedef",
        selection="#d7e4f6",
        cursor="#f29718",
        comment="#adaeb1",
        gutter="#cbd0d7",
        subtle="#828e9f",
        red="#f07171",
        green="#86b300",
        yellow="#eba400",
        blue="#22a4e6",
        magenta="#a37acc",
        cyan="#4cbf99",
        orange="#fa8532",
        purple="#a37acc",
        ansi=[
            "#000000", "#f06b6c", "#6cbf43", "#e7a100",
            "#21a1e2", "#a176cb", "#4abc96", "#c7c7c7",
            "#686868", "#f07171", "#86b300", "#eba400",
            "#22a4e6", "#a37acc", "#4cbf99", "#d1d1d1",
        ],
    ),
    "dracula-dark": Palette(
        title="Dracula",
        note=(
            "Dracula Classic, from the specification its authors publish. Dracula names no blue: "
            "its own terminal table puts the purple in the blue slot, and the blue tokens here "
            "follow it. It names one foreground and one grey, so the line numbers take the comment "
            "colour and secondary text takes the foreground."
        ),
        dark=True,
        bg="#282a36",
        fg="#f8f8f2",
        sidebar="#21222c",
        popover="#343746",
        muted="#343746",
        accent="#424450",
        selection="#44475a",
        cursor="#f8f8f2",
        comment="#6272a4",
        gutter="#6272a4",
        subtle="#f8f8f2",
        red="#ff5555",
        green="#50fa7b",
        yellow="#f1fa8c",
        blue="#bd93f9",
        magenta="#ff79c6",
        cyan="#8be9fd",
        orange="#ffb86c",
        purple="#bd93f9",
        ansi=[
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c",
            "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5",
            "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
        ],
    ),
    "dracula-light": Palette(
        title="Alucard",
        note=(
            "Alucard Classic, Dracula's own light theme, with its published grounds and its "
            "published sixteen. The same arrangement as the dark surface: purple stands where the "
            "interface asks for blue."
        ),
        dark=False,
        bg="#fffbeb",
        fg="#1f1f1f",
        sidebar="#ece9df",
        popover="#efeddc",
        muted="#efeddc",
        accent="#dedccf",
        selection="#cfcfde",
        cursor="#1f1f1f",
        comment="#6c664b",
        gutter="#6c664b",
        subtle="#2c2b31",
        red="#cb3a2a",
        green="#14710a",
        yellow="#846e15",
        blue="#644ac9",
        magenta="#a3144d",
        cyan="#036a96",
        orange="#a34d14",
        purple="#644ac9",
        ansi=[
            "#fffbeb", "#cb3a2a", "#14710a", "#846e15",
            "#644ac9", "#a3144d", "#036a96", "#1f1f1f",
            "#6c664b", "#d74c3d", "#198d0c", "#9e841a",
            "#7862d0", "#bf185a", "#047fb4", "#2c2b31",
        ],
    ),
    "edge-dark": Palette(
        title="Edge",
        note=(
            "sainnhe's, in its default style. Edge publishes no orange, so the tokens that ask for "
            "one hold the yellow, and no sixteen, so the terminal row is built from the hues."
        ),
        dark=True,
        bg="#2c2e34",
        fg="#c5cdd9",
        sidebar="#24262a",
        popover="#33353f",
        muted="#33353f",
        accent="#3b3e48",
        selection="#414550",
        cursor="#c5cdd9",
        comment="#758094",
        gutter="#535c6a",
        subtle="#758094",
        red="#ec7279",
        green="#a0c980",
        yellow="#deb974",
        blue="#6cb6eb",
        magenta="#d38aea",
        cyan="#5dbbc1",
        orange="#deb974",
        purple="#d38aea",
    ),
    "edge-light": Palette(
        title="Edge Light",
        note="The light surface of the same default style, arranged the same way.",
        dark=False,
        bg="#fafafa",
        fg="#4b505b",
        sidebar="#e8ebf0",
        popover="#eef1f4",
        muted="#eef1f4",
        accent="#e8ebf0",
        selection="#dde2e7",
        cursor="#4b505b",
        comment="#8790a0",
        gutter="#bac3cb",
        subtle="#8790a0",
        red="#d05858",
        green="#608e32",
        yellow="#be7e05",
        blue="#5079be",
        magenta="#b05ccc",
        cyan="#3a8b84",
        orange="#be7e05",
        purple="#b05ccc",
    ),
    "everforest-dark": Palette(
        title="Everforest",
        note=(
            "The dark medium contrast. The selection band is Everforest's own `bg_visual`, which "
            "is a wine tint rather than a grey, and the file tree sits on `bg_dim`."
        ),
        dark=True,
        bg="#2d353b",
        fg="#d3c6aa",
        sidebar="#232a2e",
        popover="#343f44",
        muted="#343f44",
        accent="#475258",
        selection="#543a48",
        cursor="#d3c6aa",
        comment="#859289",
        gutter="#7a8478",
        subtle="#9da9a0",
        red="#e67e80",
        green="#a7c080",
        yellow="#dbbc7f",
        blue="#7fbbb3",
        magenta="#d699b6",
        cyan="#83c092",
        orange="#e69875",
        purple="#d699b6",
    ),
    "everforest-light": Palette(
        title="Everforest Light",
        note="The light medium contrast, with the same arrangement of grounds.",
        dark=False,
        bg="#fdf6e3",
        fg="#5c6a72",
        sidebar="#efebd4",
        popover="#f4f0d9",
        muted="#f4f0d9",
        accent="#e6e2cc",
        selection="#eaedc8",
        cursor="#5c6a72",
        comment="#939f91",
        gutter="#a6b0a0",
        subtle="#829181",
        red="#f85552",
        green="#8da101",
        yellow="#dfa000",
        blue="#3a94c5",
        magenta="#df69ba",
        cyan="#35a77c",
        orange="#f57d26",
        purple="#df69ba",
    ),
    "flexoki-dark": Palette(
        title="Flexoki",
        note=(
            "Steph Ango's, from the published scale. The greys are its own ramp — black, 950, 900, "
            "850, 800 for the grounds and 700 to 200 for the text — and the hues are the 400 "
            "level, which is the one Flexoki uses on a dark surface."
        ),
        dark=True,
        bg="#100f0f",
        fg="#cecdc3",
        sidebar="#1c1b1a",
        popover="#1c1b1a",
        muted="#282726",
        accent="#343331",
        selection="#403e3c",
        cursor="#cecdc3",
        comment="#878580",
        gutter="#575653",
        subtle="#b7b5ac",
        red="#d14d41",
        green="#879a39",
        yellow="#d0a215",
        blue="#4385be",
        magenta="#ce5d97",
        cyan="#3aa99f",
        orange="#da702c",
        purple="#8b7ec8",
    ),
    "flexoki-light": Palette(
        title="Flexoki Light",
        note=(
            "The same scale on paper: the grounds run from `paper` down through 50, 100 and 150, "
            "and the hues are the 600 level Flexoki uses on a light surface."
        ),
        dark=False,
        bg="#fffcf0",
        fg="#100f0f",
        sidebar="#f2f0e5",
        popover="#f2f0e5",
        muted="#e6e4d9",
        accent="#dad8ce",
        selection="#cecdc3",
        cursor="#100f0f",
        comment="#6f6e69",
        gutter="#b7b5ac",
        subtle="#575653",
        red="#af3029",
        green="#66800b",
        yellow="#ad8301",
        blue="#205ea6",
        magenta="#a02f6f",
        cyan="#24837b",
        orange="#bc5215",
        purple="#5e409d",
    ),
    "github-dark": Palette(
        title="GitHub",
        note=(
            "GitHub Dark, from the Primer primitives: the canvas, the syntax colours GitHub calls "
            "`prettylights`, and its own sixteen. The selection is GitHub's translucent blue "
            "flattened over the canvas. GitHub colours keywords red and functions purple; here the "
            "purple carries the keywords and the blue the functions."
        ),
        dark=True,
        bg="#0d1117",
        fg="#e6edf3",
        sidebar="#010409",
        popover="#161b22",
        muted="#161b22",
        accent="#21262d",
        selection="#173355",
        cursor="#2f81f7",
        comment="#8b949e",
        gutter="#6e7681",
        subtle="#848d97",
        red="#ff7b72",
        green="#7ee787",
        yellow="#e3b341",
        blue="#79c0ff",
        magenta="#d2a8ff",
        cyan="#39c5cf",
        orange="#ffa657",
        purple="#d2a8ff",
        ansi=[
            "#484f58", "#ff7b72", "#3fb950", "#d29922",
            "#58a6ff", "#bc8cff", "#39c5cf", "#b1bac4",
            "#6e7681", "#ffa198", "#56d364", "#e3b341",
            "#79c0ff", "#d2a8ff", "#56d4dd", "#ffffff",
        ],
    ),
    "github-light": Palette(
        title="GitHub Light",
        note=(
            "GitHub Light, from the same primitives. Its terminal yellow is a very dark brown, "
            "which is GitHub's own choice and is kept."
        ),
        dark=False,
        bg="#ffffff",
        fg="#1f2328",
        sidebar="#f6f8fa",
        popover="#ffffff",
        muted="#f6f8fa",
        accent="#eaeef2",
        selection="#dae9f9",
        cursor="#0969da",
        comment="#57606a",
        gutter="#8c959f",
        subtle="#656d76",
        red="#cf222e",
        green="#116329",
        yellow="#9a6700",
        blue="#0550ae",
        magenta="#8250df",
        cyan="#1b7c83",
        orange="#953800",
        purple="#8250df",
        ansi=[
            "#24292f", "#cf222e", "#116329", "#4d2d00",
            "#0969da", "#8250df", "#1b7c83", "#6e7781",
            "#57606a", "#a40e26", "#1a7f37", "#633c01",
            "#218bff", "#a475f9", "#3192aa", "#8c959f",
        ],
    ),
    "gruvbox-material-dark": Palette(
        title="Gruvbox Material",
        note=(
            "sainnhe's softer reading of Gruvbox, in the medium background and the material "
            "foreground. Same grounds as Gruvbox, muted hues over them."
        ),
        dark=True,
        bg="#282828",
        fg="#d4be98",
        sidebar="#1b1b1b",
        popover="#32302f",
        muted="#32302f",
        accent="#45403d",
        selection="#5a524c",
        cursor="#d4be98",
        comment="#928374",
        gutter="#7c6f64",
        subtle="#ddc7a1",
        red="#ea6962",
        green="#a9b665",
        yellow="#d8a657",
        blue="#7daea3",
        magenta="#d3869b",
        cyan="#89b482",
        orange="#e78a4e",
        purple="#d3869b",
    ),
    "gruvbox-material-light": Palette(
        title="Gruvbox Material Light",
        note="The light medium background with the material foreground.",
        dark=False,
        bg="#fbf1c7",
        fg="#654735",
        sidebar="#f2e5bc",
        popover="#f4e8be",
        muted="#f2e5bc",
        accent="#eee0b7",
        selection="#ddccab",
        cursor="#654735",
        comment="#928374",
        gutter="#a89984",
        subtle="#4f3829",
        red="#c14a4a",
        green="#6c782e",
        yellow="#b47109",
        blue="#45707a",
        magenta="#945e80",
        cyan="#4c7a5d",
        orange="#c35e0a",
        purple="#945e80",
    ),
    "iceberg-dark": Palette(
        title="Iceberg",
        note=(
            "cocopon's, read out of the colourscheme itself, with its published sixteen. Iceberg "
            "names one foreground and no orange; secondary text takes the foreground and the "
            "orange tokens take the yellow."
        ),
        dark=True,
        bg="#161821",
        fg="#c6c8d1",
        sidebar="#1e2132",
        popover="#3d425b",
        muted="#1e2132",
        accent="#272c42",
        selection="#272c42",
        cursor="#c6c8d1",
        comment="#6b7089",
        gutter="#444b71",
        subtle="#c6c8d1",
        red="#e27878",
        green="#b4be82",
        yellow="#e2a478",
        blue="#84a0c6",
        magenta="#a093c7",
        cyan="#89b8c2",
        orange="#e2a478",
        purple="#a093c7",
        ansi=[
            "#1e2132", "#e27878", "#b4be82", "#e2a478",
            "#84a0c6", "#a093c7", "#89b8c2", "#c6c8d1",
            "#6b7089", "#e98989", "#c0ca8e", "#e9b189",
            "#91acd1", "#ada0d3", "#95c4ce", "#d2d4de",
        ],
    ),
    "iceberg-light": Palette(
        title="Iceberg Light",
        note="The light surface of the same colourscheme, with its own sixteen.",
        dark=False,
        bg="#e8e9ec",
        fg="#33374c",
        sidebar="#dcdfe7",
        popover="#cad0de",
        muted="#dcdfe7",
        accent="#c9cdd7",
        selection="#c9cdd7",
        cursor="#33374c",
        comment="#8389a3",
        gutter="#9fa7bd",
        subtle="#33374c",
        red="#cc517a",
        green="#668e3d",
        yellow="#c57339",
        blue="#2d539e",
        magenta="#7759b4",
        cyan="#3f83a6",
        orange="#c57339",
        purple="#7759b4",
        ansi=[
            "#dcdfe7", "#cc517a", "#668e3d", "#c57339",
            "#2d539e", "#7759b4", "#3f83a6", "#33374c",
            "#8389a3", "#cc3768", "#598030", "#b6662d",
            "#22478e", "#6845ad", "#327698", "#262a3f",
        ],
    ),
    "kanagawa-dark": Palette(
        title="Kanagawa Wave",
        note=(
            "The Wave variant, whose colours are named after the Hokusai print it comes from. The "
            "sixteen are Kanagawa's own autumn row over the spring one. Kanagawa colours types "
            "with the desaturated aqua; here they take the carp yellow, which is the tone it puts "
            "beside them."
        ),
        dark=True,
        bg="#1f1f28",
        fg="#dcd7ba",
        sidebar="#16161d",
        popover="#16161d",
        muted="#2a2a37",
        accent="#363646",
        selection="#2d4f67",
        cursor="#dcd7ba",
        comment="#727169",
        gutter="#54546d",
        subtle="#c8c093",
        red="#e46876",
        green="#98bb6c",
        yellow="#e6c384",
        blue="#7e9cd8",
        magenta="#957fb8",
        cyan="#7fb4ca",
        orange="#ffa066",
        purple="#957fb8",
        ansi=[
            "#16161d", "#c34043", "#76946a", "#c0a36e",
            "#7e9cd8", "#957fb8", "#6a9589", "#c8c093",
            "#727169", "#e82424", "#98bb6c", "#e6c384",
            "#7fb4ca", "#938aa9", "#7aa89f", "#dcd7ba",
        ],
    ),
    "kanagawa-light": Palette(
        title="Kanagawa Lotus",
        note=(
            "The Lotus variant, the same palette turned onto paper. The grounds are its lotus "
            "whites and the text its lotus inks. Lotus publishes no terminal row, so the sixteen "
            "are built from the hues."
        ),
        dark=False,
        bg="#f2ecbc",
        fg="#545464",
        sidebar="#e7dba0",
        popover="#e7dba0",
        muted="#e5ddb0",
        accent="#dcd5ac",
        selection="#d5cea3",
        cursor="#43436c",
        comment="#8a8980",
        gutter="#a09cac",
        subtle="#716e61",
        red="#c84053",
        green="#6f894e",
        yellow="#836f4a",
        blue="#4d699b",
        magenta="#624c83",
        cyan="#597b75",
        orange="#cc6d00",
        purple="#624c83",
    ),
    "material-dark": Palette(
        title="Material Palenight",
        note=(
            "The Material theme in its Palenight style, from the neovim port's colour table. The "
            "cursor is Material's own amber accent rather than the foreground."
        ),
        dark=True,
        bg="#292d3e",
        fg="#a6accd",
        sidebar="#1b1e2b",
        popover="#202331",
        muted="#414863",
        accent="#444267",
        selection="#444267",
        cursor="#ffcc00",
        comment="#676e95",
        gutter="#3a3f58",
        subtle="#717cb4",
        red="#f07178",
        green="#c3e88d",
        yellow="#ffcb6b",
        blue="#82aaff",
        magenta="#c792ea",
        cyan="#89ddff",
        orange="#f78c6c",
        purple="#c792ea",
    ),
    "material-light": Palette(
        title="Material Lighter",
        note=(
            "The Lighter style of the same theme. Its selection is the published teal, which is "
            "the one thing in Material that is a colour rather than a grey."
        ),
        dark=False,
        bg="#fafafa",
        fg="#546e7a",
        sidebar="#eeeeee",
        popover="#ffffff",
        muted="#e7e7e8",
        accent="#e7e7e8",
        selection="#80cbc4",
        cursor="#272727",
        comment="#aabfc9",
        gutter="#cfd8dc",
        subtle="#94a7b0",
        red="#e53935",
        green="#91b859",
        yellow="#f6a434",
        blue="#6182b8",
        magenta="#7c4dff",
        cyan="#39adb5",
        orange="#f76d47",
        purple="#7c4dff",
    ),
    "melange-dark": Palette(
        title="Melange",
        note=(
            "savq's, from its own palette files. Melange names two levels of every hue; the "
            "brighter one carries the syntax and the darker one stands where an orange is asked "
            "for, since it publishes none. Its comments are brighter than its interface grey, "
            "which is the theme's decision and is kept."
        ),
        dark=True,
        bg="#292522",
        fg="#ece1d7",
        sidebar="#34302c",
        popover="#34302c",
        muted="#34302c",
        accent="#403a36",
        selection="#403a36",
        cursor="#ece1d7",
        comment="#c1a78e",
        gutter="#867462",
        subtle="#c1a78e",
        red="#d47766",
        green="#85b695",
        yellow="#ebc06d",
        blue="#a3a9ce",
        magenta="#cf9bc2",
        cyan="#89b3b6",
        orange="#e49b5d",
        purple="#cf9bc2",
    ),
    "melange-light": Palette(
        title="Melange Light",
        note="The light surface of the same palette, arranged the same way.",
        dark=False,
        bg="#f1f1f1",
        fg="#54433a",
        sidebar="#e9e1db",
        popover="#e9e1db",
        muted="#e9e1db",
        accent="#d9d3ce",
        selection="#d9d3ce",
        cursor="#54433a",
        comment="#7d6658",
        gutter="#a98a78",
        subtle="#7d6658",
        red="#bf0021",
        green="#3a684a",
        yellow="#a06d00",
        blue="#465aa4",
        magenta="#904180",
        cyan="#3d6568",
        orange="#bc5c00",
        purple="#904180",
    ),
    "modus-dark": Palette(
        title="Modus Vivendi",
        note=(
            "Protesilaos Stavrou's, by way of the neovim port. Modus is written to a contrast "
            "ratio rather than to a mood: the ground is black, the text is white, and every hue on "
            "it is at least seven to one against the ground. Nothing here is softened."
        ),
        dark=True,
        bg="#000000",
        fg="#ffffff",
        sidebar="#0f0f0f",
        popover="#1e1e1e",
        muted="#1e1e1e",
        accent="#303030",
        selection="#303030",
        cursor="#ffffff",
        comment="#989898",
        gutter="#646464",
        subtle="#c4c4c4",
        red="#ff5f59",
        green="#44bc44",
        yellow="#d0bc00",
        blue="#2fafff",
        magenta="#feacd0",
        cyan="#00d3d0",
        orange="#fec43f",
        purple="#b6a0ff",
    ),
    "modus-light": Palette(
        title="Modus Operandi",
        note=(
            "The light half of the same pair, and the same rule: white ground, black text, and "
            "hues dark enough to hold seven to one against it."
        ),
        dark=False,
        bg="#ffffff",
        fg="#000000",
        sidebar="#f0f0f0",
        popover="#f2f2f2",
        muted="#f2f2f2",
        accent="#e0e0e0",
        selection="#e0e0e0",
        cursor="#000000",
        comment="#595959",
        gutter="#9f9f9f",
        subtle="#3b3b3b",
        red="#a60000",
        green="#006800",
        yellow="#6f5500",
        blue="#0031a9",
        magenta="#721045",
        cyan="#005e8b",
        orange="#884900",
        purple="#531ab6",
    ),
    "monokai-pro-dark": Palette(
        title="Monokai Pro",
        note=(
            "The Pro filter, from its six accents and its five dimmed greys. Monokai colours "
            "strings yellow and functions green; the arrangement here keeps every hue where its "
            "name says it belongs, so strings are green, functions are the accent cyan and "
            "keywords the accent purple."
        ),
        dark=True,
        bg="#2d2a2e",
        fg="#fcfcfa",
        sidebar="#221f22",
        popover="#221f22",
        muted="#403e41",
        accent="#403e41",
        selection="#5b595c",
        cursor="#fcfcfa",
        comment="#727072",
        gutter="#5b595c",
        subtle="#c1c0c0",
        red="#ff6188",
        green="#a9dc76",
        yellow="#ffd866",
        blue="#78dce8",
        magenta="#ab9df2",
        cyan="#78dce8",
        orange="#fc9867",
        purple="#ab9df2",
    ),
    "monokai-pro-light": Palette(
        title="Monokai Pro Light",
        note="The Light filter of the same theme, with the accents it darkens for paper.",
        dark=False,
        bg="#faf4f2",
        fg="#29242a",
        sidebar="#ede7e5",
        popover="#ede7e5",
        muted="#d3cdcc",
        accent="#d3cdcc",
        selection="#bfb9ba",
        cursor="#29242a",
        comment="#a59fa0",
        gutter="#bfb9ba",
        subtle="#706b6e",
        red="#e14775",
        green="#269d69",
        yellow="#cc7a0a",
        blue="#1c8ca8",
        magenta="#7058be",
        cyan="#1c8ca8",
        orange="#e16032",
        purple="#7058be",
    ),
    "moonfly-dark": Palette(
        title="Moonfly",
        note=(
            "bluz71's, from the palette in its own source, terminal row and all. A near-black "
            "ground and greys named by their lightness, with one saturated hue for each thing the "
            "syntax has to tell apart."
        ),
        dark=True,
        bg="#080808",
        fg="#c6c6c6",
        sidebar="#121212",
        popover="#212121",
        muted="#1c1c1c",
        accent="#262626",
        selection="#323437",
        cursor="#9e9e9e",
        comment="#949494",
        gutter="#626262",
        subtle="#b2b2b2",
        red="#ff5d5d",
        green="#8cc85f",
        yellow="#e3c78a",
        blue="#80a0ff",
        magenta="#cf87e8",
        cyan="#79dac8",
        orange="#de935f",
        purple="#ae81ff",
        ansi=[
            "#323437", "#ff5d5d", "#8cc85f", "#e3c78a",
            "#80a0ff", "#cf87e8", "#79dac8", "#c6c6c6",
            "#949494", "#ff5189", "#36c692", "#c6c684",
            "#74b2ff", "#ae81ff", "#85dc85", "#e4e4e4",
        ],
    ),
    "night-owl-dark": Palette(
        title="Night Owl",
        note=(
            "Sarah Drasner's, read from the VS Code theme itself. Night Owl colours strings the "
            "warm tan it uses for text; here they take the lime it gives to variables, because "
            "this palette keeps one hue per kind of thing."
        ),
        dark=True,
        bg="#011627",
        fg="#d6deeb",
        sidebar="#01111d",
        popover="#021320",
        muted="#072435",
        accent="#0e293f",
        selection="#1d3b53",
        cursor="#80a4c2",
        comment="#637777",
        gutter="#4b6479",
        subtle="#7e97ac",
        red="#ef5350",
        green="#c5e478",
        yellow="#ffeb95",
        blue="#82aaff",
        magenta="#c792ea",
        cyan="#7fdbca",
        orange="#f78c6c",
        purple="#c792ea",
        ansi=[
            "#011627", "#ef5350", "#22da6e", "#c5e478",
            "#82aaff", "#c792ea", "#21c7a8", "#ffffff",
            "#575656", "#ef5350", "#22da6e", "#ffeb95",
            "#82aaff", "#c792ea", "#7fdbca", "#ffffff",
        ],
    ),
    "night-owl-light": Palette(
        title="Light Owl",
        note="Light Owl, the daytime half of the same theme, with its own sixteen.",
        dark=False,
        bg="#fbfbfb",
        fg="#403f53",
        sidebar="#f0f0f0",
        popover="#f0f0f0",
        muted="#f0f0f0",
        accent="#e0e0e0",
        selection="#e0e0e0",
        cursor="#90a7b2",
        comment="#989fb1",
        gutter="#90a7b2",
        subtle="#90a7b2",
        red="#de3d3b",
        green="#08916a",
        yellow="#e0af02",
        blue="#4876d6",
        magenta="#994cc3",
        cyan="#2aa298",
        orange="#aa0982",
        purple="#994cc3",
        ansi=[
            "#403f53", "#de3d3b", "#08916a", "#e0af02",
            "#288ed7", "#d6438a", "#2aa298", "#93a1a1",
            "#403f53", "#de3d3b", "#08916a", "#daaa01",
            "#288ed7", "#d6438a", "#2aa298", "#93a1a1",
        ],
    ),
    "nightfly-dark": Palette(
        title="Nightfly",
        note=(
            "bluz71's other one: the same arrangement as Moonfly over a deep blue ground, with its "
            "published terminal row."
        ),
        dark=True,
        bg="#011627",
        fg="#c3ccdc",
        sidebar="#081e2f",
        popover="#09243a",
        muted="#092236",
        accent="#0e293f",
        selection="#1d3b53",
        cursor="#a1aab8",
        comment="#7c8f8f",
        gutter="#4b6479",
        subtle="#acb4c2",
        red="#fc514e",
        green="#a1cd5e",
        yellow="#e3d18a",
        blue="#82aaff",
        magenta="#c792ea",
        cyan="#7fdbca",
        orange="#f78c6c",
        purple="#ae81ff",
        ansi=[
            "#1d3b53", "#fc514e", "#a1cd5e", "#e3d18a",
            "#82aaff", "#c792ea", "#7fdbca", "#c3ccdc",
            "#7c8f8f", "#ff5874", "#21c7a8", "#ecc48d",
            "#87bcff", "#ae81ff", "#85dc85", "#d6deeb",
        ],
    ),
    "nightfox-dark": Palette(
        title="Nightfox",
        note=(
            "EdenEast's, from the palette its port publishes. The bright half of its terminal row "
            "is computed from the normal half rather than written down, so the sixteen here are "
            "built from the hues."
        ),
        dark=True,
        bg="#192330",
        fg="#cdcecf",
        sidebar="#131a24",
        popover="#212e3f",
        muted="#212e3f",
        accent="#29394f",
        selection="#2b3b51",
        cursor="#cdcecf",
        comment="#738091",
        gutter="#71839b",
        subtle="#aeafb0",
        red="#c94f6d",
        green="#81b29a",
        yellow="#dbc074",
        blue="#719cd6",
        magenta="#9d79d6",
        cyan="#63cdcf",
        orange="#f4a261",
        purple="#9d79d6",
    ),
    "nightfox-light": Palette(
        title="Dayfox",
        note="Dayfox, the light member of the same family, from its own palette.",
        dark=False,
        bg="#f6f2ee",
        fg="#3d2b5a",
        sidebar="#e4dcd4",
        popover="#dbd1dd",
        muted="#dbd1dd",
        accent="#d3c7bb",
        selection="#e7d2be",
        cursor="#3d2b5a",
        comment="#837a72",
        gutter="#824d5b",
        subtle="#643f61",
        red="#a5222f",
        green="#396847",
        yellow="#ac5402",
        blue="#2848a9",
        magenta="#6e33ce",
        cyan="#287980",
        orange="#955f61",
        purple="#6e33ce",
    ),
    "nord-dark": Palette(
        title="Nord",
        note=(
            "Arctic Ice Studio's, with its published terminal row. Nord's Snow Storm has three "
            "levels, so the brightest carries the text and the one below it the secondary text. "
            "Nord colours keywords with the frost blue and numbers with the aurora purple; the "
            "arrangement here is the other way round, because this palette gives functions the "
            "blue."
        ),
        dark=True,
        bg="#2e3440",
        fg="#eceff4",
        sidebar="#2e3440",
        popover="#3b4252",
        muted="#3b4252",
        accent="#434c5e",
        selection="#434c5e",
        cursor="#d8dee9",
        comment="#616e88",
        gutter="#4c566a",
        subtle="#d8dee9",
        red="#bf616a",
        green="#a3be8c",
        yellow="#ebcb8b",
        blue="#81a1c1",
        magenta="#b48ead",
        cyan="#88c0d0",
        orange="#d08770",
        purple="#b48ead",
        ansi=[
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b",
            "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
            "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b",
            "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
        ],
    ),
    "one-dark": Palette(
        title="One Dark",
        note=(
            "Atom's, which most of the editors written since have shipped a copy of. The cursor is "
            "the blue Atom's own syntax package names; everything else is the palette."
        ),
        dark=True,
        bg="#282c34",
        fg="#abb2bf",
        sidebar="#21252b",
        popover="#21252b",
        muted="#31353f",
        accent="#393f4a",
        selection="#3b3f4c",
        cursor="#528bff",
        comment="#5c6370",
        gutter="#4b5263",
        subtle="#848b98",
        red="#e06c75",
        green="#98c379",
        yellow="#e5c07b",
        blue="#61afef",
        magenta="#c678dd",
        cyan="#56b6c2",
        orange="#d19a66",
        purple="#c678dd",
    ),
    "one-light": Palette(
        title="One Light",
        note=(
            "The light half of the same pair. Atom gives classes the yellow and constants the "
            "darker gold, and the type and constant tokens here follow it."
        ),
        dark=False,
        bg="#fafafa",
        fg="#383a42",
        sidebar="#f0f0f0",
        popover="#f0f0f0",
        muted="#f0f0f0",
        accent="#e6e6e6",
        selection="#dcdcdc",
        cursor="#526fff",
        comment="#a0a1a7",
        gutter="#9d9d9f",
        subtle="#818387",
        red="#e45649",
        green="#50a14f",
        yellow="#c18401",
        blue="#4078f2",
        magenta="#a626a4",
        cyan="#0184bc",
        orange="#986801",
        purple="#a626a4",
    ),
    "oxocarbon-dark": Palette(
        title="Oxocarbon",
        note=(
            "Nyoom Engineering's, built on IBM's Carbon design colours. Like Vesper it is a short "
            "palette: there is no yellow and no orange in it, so the tokens that name those hold "
            "the purple Carbon uses for a warning and the light blue it uses for a number. The "
            "greys are Carbon's own ramp and the sixteen are Oxocarbon's own table, which is why "
            "its terminal green is a purple."
        ),
        dark=True,
        bg="#161616",
        fg="#dde1e6",
        sidebar="#131313",
        popover="#262626",
        muted="#262626",
        accent="#393939",
        selection="#393939",
        cursor="#dde1e6",
        comment="#525252",
        gutter="#525252",
        subtle="#dde1e6",
        red="#ee5396",
        green="#42be65",
        yellow="#be95ff",
        blue="#78a9ff",
        magenta="#ff7eb6",
        cyan="#3ddbd9",
        orange="#82cfff",
        purple="#be95ff",
        ansi=[
            "#262626", "#33b1ff", "#be95ff", "#42be65",
            "#78a9ff", "#82cfff", "#3ddbd9", "#f2f4f8",
            "#525252", "#33b1ff", "#be95ff", "#42be65",
            "#78a9ff", "#82cfff", "#08bdba", "#ffffff",
        ],
    ),
    "oxocarbon-light": Palette(
        title="Oxocarbon Light",
        note=(
            "The light surface, which reaches further outside Carbon's greys than the dark one "
            "does. Oxocarbon paints its comments with the near-black it uses as a ground there, "
            "which would make them the loudest thing on the screen; they take the blue-grey it "
            "publishes beside it instead."
        ),
        dark=False,
        bg="#ffffff",
        fg="#37474f",
        sidebar="#f2f4f8",
        popover="#f2f4f8",
        muted="#f2f4f8",
        accent="#dde1e6",
        selection="#dde1e6",
        cursor="#37474f",
        comment="#90a4ae",
        gutter="#90a4ae",
        subtle="#525252",
        red="#ee5396",
        green="#42be65",
        yellow="#ffab91",
        blue="#0f62fe",
        magenta="#ff7eb6",
        cyan="#08bdba",
        orange="#ff6f00",
        purple="#673ab7",
    ),
    "papercolor-dark": Palette(
        title="PaperColor Dark",
        note=(
            "NLKNguyen's, whose palette is written in the two hundred and fifty-six colours a "
            "terminal has and looks it: every value is one of them exactly."
        ),
        dark=True,
        bg="#1c1c1c",
        fg="#d0d0d0",
        sidebar="#262626",
        popover="#303030",
        muted="#303030",
        accent="#3a3a3a",
        selection="#4e4e4e",
        cursor="#c6c6c6",
        comment="#808080",
        gutter="#585858",
        subtle="#bcbcbc",
        red="#af005f",
        green="#5faf00",
        yellow="#d7af5f",
        blue="#5fafd7",
        magenta="#af87d7",
        cyan="#00afaf",
        orange="#d7875f",
        purple="#af87d7",
    ),
    "papercolor-light": Palette(
        title="PaperColor",
        note=(
            "The light surface it is named for, and the one its author designed first: paper, ink, "
            "and saturated hues that stay legible on it."
        ),
        dark=False,
        bg="#eeeeee",
        fg="#444444",
        sidebar="#e4e4e4",
        popover="#d0d0d0",
        muted="#e4e4e4",
        accent="#d0d0d0",
        selection="#bcbcbc",
        cursor="#005f87",
        comment="#878787",
        gutter="#b2b2b2",
        subtle="#666666",
        red="#af0000",
        green="#008700",
        yellow="#af5f00",
        blue="#005f87",
        magenta="#8700af",
        cyan="#0087af",
        orange="#d75f00",
        purple="#8700af",
    ),
    "poimandres-dark": Palette(
        title="Poimandres",
        note=(
            "Oliver Cederborg's port of the theme by pmndrs. Another short palette: a teal, four "
            "blues, three pinks and one yellow, with no red and no green among them. The tokens "
            "that name those hold the pink and the teal, which is what the theme itself uses in "
            "their place."
        ),
        dark=True,
        bg="#1b1e28",
        fg="#e4f0fb",
        sidebar="#171922",
        popover="#171922",
        muted="#303340",
        accent="#303340",
        selection="#506477",
        cursor="#e4f0fb",
        comment="#767c9d",
        gutter="#506477",
        subtle="#a6accd",
        red="#d0679d",
        green="#5de4c7",
        yellow="#fffac2",
        blue="#add7ff",
        magenta="#91b4d5",
        cyan="#89ddff",
        orange="#fcc5e9",
        purple="#fae4fc",
    ),
    "solarized-dark": Palette(
        title="Solarized Dark",
        note=(
            "Ethan Schoonover's, and the oldest palette here. Its eight monotones carry the "
            "grounds and the text, its eight accents carry everything else, and both surfaces use "
            "the same accents — which is the whole point of it. The terminal row is Solarized's "
            "own, where the bright half holds the monotones rather than brighter accents."
        ),
        dark=True,
        bg="#002b36",
        fg="#839496",
        sidebar="#073642",
        popover="#073642",
        muted="#073642",
        accent="#073642",
        selection="#073642",
        cursor="#93a1a1",
        comment="#586e75",
        gutter="#586e75",
        subtle="#93a1a1",
        red="#dc322f",
        green="#859900",
        yellow="#b58900",
        blue="#268bd2",
        magenta="#d33682",
        cyan="#2aa198",
        orange="#cb4b16",
        purple="#6c71c4",
        ansi=[
            "#073642", "#dc322f", "#859900", "#b58900",
            "#268bd2", "#d33682", "#2aa198", "#eee8d5",
            "#002b36", "#cb4b16", "#586e75", "#657b83",
            "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
        ],
    ),
    "solarized-light": Palette(
        title="Solarized Light",
        note=(
            "The same eight accents over the light monotones, which is how Solarized was meant to "
            "be read: one theme, two surfaces, and the colours of the code unchanged between them."
        ),
        dark=False,
        bg="#fdf6e3",
        fg="#657b83",
        sidebar="#eee8d5",
        popover="#eee8d5",
        muted="#eee8d5",
        accent="#eee8d5",
        selection="#eee8d5",
        cursor="#586e75",
        comment="#93a1a1",
        gutter="#93a1a1",
        subtle="#586e75",
        red="#dc322f",
        green="#859900",
        yellow="#b58900",
        blue="#268bd2",
        magenta="#d33682",
        cyan="#2aa198",
        orange="#cb4b16",
        purple="#6c71c4",
        ansi=[
            "#eee8d5", "#dc322f", "#859900", "#b58900",
            "#268bd2", "#d33682", "#2aa198", "#073642",
            "#fdf6e3", "#cb4b16", "#93a1a1", "#839496",
            "#657b83", "#6c71c4", "#586e75", "#002b36",
        ],
    ),
    "sonokai-dark": Palette(
        title="Sonokai",
        note=(
            "sainnhe's Monokai, in its default style. It publishes one blue, which is cyan enough "
            "to stand in both places, and no separate sixteen."
        ),
        dark=True,
        bg="#2c2e34",
        fg="#e2e2e3",
        sidebar="#222327",
        popover="#33353f",
        muted="#33353f",
        accent="#3b3e48",
        selection="#414550",
        cursor="#e2e2e3",
        comment="#7f8490",
        gutter="#595f6f",
        subtle="#7f8490",
        red="#fc5d7c",
        green="#9ed072",
        yellow="#e7c664",
        blue="#76cce0",
        magenta="#b39df3",
        cyan="#76cce0",
        orange="#f39660",
        purple="#b39df3",
        ansi=[
            "#181819", "#fc5d7c", "#9ed072", "#e7c664",
            "#76cce0", "#b39df3", "#76cce0", "#e2e2e3",
            "#7f8490", "#ff6077", "#a7df78", "#e7c664",
            "#85d3f2", "#b39df3", "#76cce0", "#e2e2e3",
        ],
    ),
    "vitesse-dark": Palette(
        title="Vitesse",
        note=(
            "Anthony Fu's, read from the VS Code theme. Vitesse writes its selection, its line "
            "numbers and its foreground with an alpha over a flat near-black; the values here are "
            "those colours flattened over it. It colours keywords with the green it gives to "
            "strings' neighbours, and the tan that carries variables stands where an orange is "
            "asked for."
        ),
        dark=True,
        bg="#121212",
        fg="#dbd7ca",
        sidebar="#121212",
        popover="#181818",
        muted="#181818",
        accent="#272727",
        selection="#272727",
        cursor="#dbd7ca",
        comment="#758575",
        gutter="#52514f",
        subtle="#bfbaaa",
        red="#cb7676",
        green="#4d9375",
        yellow="#e6cc77",
        blue="#6394bf",
        magenta="#d9739f",
        cyan="#5eaab5",
        orange="#bd976a",
        purple="#d9739f",
        ansi=[
            "#393a34", "#cb7676", "#4d9375", "#e6cc77",
            "#6394bf", "#d9739f", "#5eaab5", "#dbd7ca",
            "#777777", "#cb7676", "#4d9375", "#e6cc77",
            "#6394bf", "#d9739f", "#5eaab5", "#ffffff",
        ],
    ),
    "vitesse-light": Palette(
        title="Vitesse Light",
        note="The light surface of the same theme, flattened the same way.",
        dark=False,
        bg="#ffffff",
        fg="#393a34",
        sidebar="#ffffff",
        popover="#f7f7f7",
        muted="#f7f7f7",
        accent="#eaeaea",
        selection="#eaeaea",
        cursor="#393a34",
        comment="#a0ada0",
        gutter="#c1c1bf",
        subtle="#4e4f47",
        red="#ab5959",
        green="#1e754f",
        yellow="#bda437",
        blue="#296aa3",
        magenta="#a13865",
        cyan="#2993a3",
        orange="#b07d48",
        purple="#a13865",
        ansi=[
            "#121212", "#ab5959", "#1e754f", "#bda437",
            "#296aa3", "#a13865", "#2993a3", "#dbd7ca",
            "#aaaaaa", "#ab5959", "#1e754f", "#bda437",
            "#296aa3", "#a13865", "#2993a3", "#dddddd",
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
