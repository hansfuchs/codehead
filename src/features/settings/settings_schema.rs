use super::Spec;

pub(super) const fn spec(key: &'static str, label: &'static str, help: &'static str, env: &'static str) -> Spec {
    Spec { key, label, help, env }
}

/// The plain fields, then the alias boxes.
pub(super) const SPECS: [Spec; 34] = [
    spec(
        "notes",
        "notes",
        "directory of Markdown notes, ~ expands to home",
        "CODEHEAD_NOTES",
    ),
    spec(
        "styles",
        "styles",
        "directory of agent output styles",
        "CODEHEAD_STYLES",
    ),
    spec(
        "home",
        "home repo",
        "git repo with AGENTS.md (notes, other repos): every agent reads it",
        "CODEHEAD_HOME_REPO",
    ),
    spec(
        "repo",
        "repo",
        "repo of the new tab's where row, ~ expands to home; empty: the repo of the dir codehead starts in",
        "CODEHEAD_REPO",
    ),
    spec("agent", "agent", "agent picked for a new thread", "CODEHEAD_AGENT"),
    spec(
        "prefix",
        "prefix",
        "prefix of new branches and worktrees",
        "CODEHEAD_PREFIX",
    ),
    spec(
        "ports",
        "ports",
        "dev servers: auto (portless when installed), portless, or block ($CODEHEAD_PORT only)",
        "CODEHEAD_PORTS",
    ),
    spec(
        "messages",
        "messages",
        "bubbles (boxes, yours on the right) or lines (name, bar)",
        "CODEHEAD_MESSAGES",
    ),
    spec(
        "thoughts",
        "thoughts",
        "hidden, folded (a click opens) or open · ctrl+t in a thread",
        "CODEHEAD_THOUGHTS",
    ),
    spec(
        "edits",
        "your edits",
        "what an agent gets told of edits since its last turn: off, files or diff",
        "CODEHEAD_EDITS",
    ),
    spec(
        "classify",
        "classifier",
        "changes view tabs (critical · high · medium · low) + order: an agent, empty: the default agent, off: path rules",
        "CODEHEAD_CLASSIFY",
    ),
    spec(
        "rank",
        "review order",
        "changes view: claude:<model> when 0 classifier agent is found, off: path rules",
        "CODEHEAD_RANK",
    ),
    spec(
        "review",
        "reviewer",
        "changes view V: claude:<model> reviews the diff (read-only tools), off: 0 reviews",
        "CODEHEAD_REVIEW",
    ),
    spec(
        "editor",
        "editor",
        "IDE for e (changes, files) and menu → IDE: auto, or code, codium, cursor, zed, idea, flow …, terminal ($EDITOR), or a path",
        "CODEHEAD_EDITOR",
    ),
    spec(
        "previews",
        "previews",
        "files view: images + pdf drawn as images: auto (Ghostty, kitty, Rio, iTerm2, WezTerm, sixel terminals), kitty, iterm2, sixel, or off",
        "CODEHEAD_PREVIEWS",
    ),
    spec(
        "keys.menu",
        "menu",
        "opens the menu: F7-F12, ctrl+<letter>, alt+<letter> (not 1 codehead binds already)",
        "CODEHEAD_MENU_KEY",
    ),
    spec(
        "keys.command",
        "commands",
        "opens the command dropdown in a prompt: 1 symbol",
        "CODEHEAD_COMMAND_KEY",
    ),
    spec(
        "colors.scheme",
        "scheme",
        "preset: lavender, coral or amber",
        "CODEHEAD_SCHEME",
    ),
    spec(
        "colors.accent",
        "accent",
        "focus and selection: the cursor row, the active tab, the focused border",
        "",
    ),
    spec("colors.on_accent", "on_accent", "text on an accent pill", ""),
    spec(
        "colors.base",
        "base",
        "the terminal bg the colors assume: tints mix into it (0 cell painted)",
        "",
    ),
    spec("colors.surface0", "surface0", "inputs, unfocused tabs and rows", ""),
    spec(
        "colors.surface1",
        "surface1",
        "the row under the mouse, the input bubble's border",
        "",
    ),
    spec("colors.surface2", "surface2", "bars, tracks", ""),
    spec("colors.overlay0", "overlay0", "borders, dividers", ""),
    spec("colors.subtext0", "subtext0", "hints, labels, times", ""),
    spec("colors.text", "text", "normal text", ""),
    spec("colors.red", "red", "errors, failed, deleted, closed, critical", ""),
    spec(
        "colors.peach",
        "peach",
        "caution: quota 70%+, high findings, unsaved, shell mode",
        "",
    ),
    spec(
        "colors.yellow",
        "yellow",
        "waits on you: blocked, questions, gates, unpushed",
        "",
    ),
    spec("colors.green", "green", "done, passed, added, open PRs", ""),
    spec("colors.teal", "teal", "renamed files", ""),
    spec(
        "colors.sky",
        "sky",
        "under way and references: running checks, links, code, modified files",
        "",
    ),
    spec("colors.mauve", "mauve", "merged PRs", ""),
];

#[cfg(test)]
pub(crate) const PLAIN: usize = SPECS.len();

/// Groups: the 1st field, the title, what the group sets.
pub(super) const SECTIONS: [(usize, &str, &str); 5] = [
    (
        0,
        "paths",
        "where codehead reads notes, output styles and the home repo",
    ),
    (3, "new threads", "defaults of the new tab and the agent form"),
    (
        7,
        "thread view",
        "how messages and thoughts look, your edits, the review order, the IDE, previews",
    ),
    (15, "keys", "keys of every screen"),
    (17, "colors", "a preset; a set field below overrides 1 of its colors"),
];

/// The alias group, after the plain fields.
pub(super) const ALIASES: (&str, &str) = (
    "path aliases",
    "short names for directories: name/ → target/ · enter: edit / done · d: delete · enter on + add: add",
);

/// Fields and boxes: this many columns right of the group's heading.
pub(super) const INDENT: u16 = 2;
