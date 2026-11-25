use super::{Instant, KeyCode, KeyEvent, KeyModifiers, Op, Outcome, Pos, SAME_PRESS, TextArea, clip, keylog, line_mod};

impl TextArea {
    pub(super) fn key_at(&mut self, key: KeyEvent, now: Instant) -> Outcome {
        keylog(&key, now);
        let space = key.code == KeyCode::Char(' ') && key.modifiers.difference(KeyModifiers::SHIFT).is_empty();

        // the 2nd space as a key right after the burst: the 2 spaces are in already
        if space && self.period_undone.take().is_some_and(|last| now - last <= SAME_PRESS) {
            return Outcome::Handled;
        }

        self.period_undone = None;
        self.recent.push((now, key.code));

        if self.recent.len() > 5 {
            self.recent.remove(0);
        }

        let out = self.key_inner(key);

        if space {
            self.undo_auto_period(now);
        }

        out
    }

    pub(super) fn key_inner(&mut self, key: KeyEvent) -> Outcome {
        edit_key(key, self)
    }
}

fn cut_selection(ta: &mut TextArea) {
    if let Some(text) = ta.selected_text() {
        clip::copy(&text);
        ta.snapshot(Op::Other);
        ta.delete_selection();
    }
}

fn command_key(ta: &mut TextArea, key: KeyEvent, character: char) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let sup = key.modifiers.contains(KeyModifiers::SUPER);

    match character.to_ascii_lowercase() {
        'a' if shift || sup => ta.select_all(),
        // ⌘← / ⌘→ in macOS terminals arrive as ctrl+a / ctrl+e (readline): row start / end
        'a' => ta.line_edge(false, false),
        'c' => {
            if let Some(text) = ta.selected_text() {
                clip::copy(&text);
            }
        }
        'x' => {
            cut_selection(ta);
        }
        'v' => {
            if let Some(text) = clip::paste() {
                ta.insert(&text);
            }
        }
        'z' if shift || character == 'Z' => ta.restore(false),
        'z' => ta.restore(true),
        'y' => ta.restore(false),
        // readline keys: ctrl only (⌘ + these = 0 effect)
        'e' | 'w' | 'u' | 'k' | 'h' if !ctrl => return Outcome::Ignored,
        'e' => ta.line_edge(true, false),
        // ctrl+⌫ in terminals without the kitty protocol: ^H
        'w' | 'h' => ta.backspace(true),
        // ⌘⌫ in macOS terminals (ctrl+u, readline): delete to the start of the row; ctrl+k: to its end
        'u' => ta.kill_to_edge(false),
        'k' => ta.kill_to_edge(true),
        _ => return Outcome::Ignored,
    }

    Outcome::Handled
}

fn edit_key(key: KeyEvent, ta: &mut TextArea) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // ⌘ (kitty protocol: super) = ctrl for the edit keys (undo, redo, all, copy, cut, paste)
    let sup = key.modifiers.contains(KeyModifiers::SUPER);
    let page = (ta.area.height.max(2) - 1) as isize;

    if !matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
    ) {
        ta.want_x = None;
    }

    match key.code {
        KeyCode::Char(character) if ctrl || sup => return command_key(ta, key, character),
        // macOS terminals send option+←/→ as ESC b / ESC f (meta), option+⌫ as ESC DEL, option+fn+⌫ as ESC d
        KeyCode::Char('b') if alt => ta.move_to(ta.word_left(ta.cur), shift),
        KeyCode::Char('f') if alt => ta.move_to(ta.word_right(ta.cur), shift),
        KeyCode::Char('d') if alt => ta.delete(true),
        KeyCode::Char('B') if alt => ta.move_to(ta.word_left(ta.cur), true),
        KeyCode::Char('F') if alt => ta.move_to(ta.word_right(ta.cur), true),
        KeyCode::Char(' ') if !alt && ta.indent_item() => {}
        KeyCode::Char(ch) if !alt => ta.insert(&ch.to_string()),
        KeyCode::Enter if !ta.single && !ctrl && !alt => ta.newline(),
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => horizontal_key(key, ta),
        KeyCode::Up if !ta.single => ta.vertical(-1, shift),
        KeyCode::Down if !ta.single => ta.vertical(1, shift),
        KeyCode::PageUp if !ta.single => ta.vertical(-page, shift),
        KeyCode::PageDown if !ta.single => ta.vertical(page, shift),
        KeyCode::Home if ctrl => ta.move_to(Pos::default(), shift),
        KeyCode::End if ctrl => {
            let last = ta.lines.len() - 1;

            ta.move_to(
                Pos {
                    row: last,
                    col: ta.lines[last].len(),
                },
                shift,
            );
        }
        KeyCode::Home => ta.line_edge(false, shift),
        KeyCode::End => ta.line_edge(true, shift),
        _ => return Outcome::Ignored,
    }

    Outcome::Handled
}

fn horizontal_key(key: KeyEvent, ta: &mut TextArea) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        // the row: ⌘ (kitty protocol: super), alt off macOS. ⌫: from the cursor to the row's
        // start, ←→: to the row's start / end
        KeyCode::Backspace if line_mod(key.modifiers) => ta.kill_to_edge(false),
        KeyCode::Delete if line_mod(key.modifiers) => ta.kill_to_edge(true),
        KeyCode::Left if line_mod(key.modifiers) => ta.line_edge(false, shift),
        KeyCode::Right if line_mod(key.modifiers) => ta.line_edge(true, shift),
        // the string: ctrl, ⌥ (= alt on macOS). ⌫: the string left of the cursor (in a path:
        // 1 segment), ←→: word jumps
        KeyCode::Backspace => ta.backspace(ctrl || alt),
        KeyCode::Delete => ta.delete(ctrl || alt),
        KeyCode::Left if ctrl || alt => ta.move_to(ta.word_left(ta.cur), shift),
        KeyCode::Right if ctrl || alt => ta.move_to(ta.word_right(ta.cur), shift),
        KeyCode::Left => match (shift, ta.range()) {
            (false, Some((start, _))) => ta.move_to(start, false),
            _ => ta.move_to(ta.left_of(ta.cur), shift),
        },
        KeyCode::Right => match (shift, ta.range()) {
            (false, Some((_, bytes))) => ta.move_to(bytes, false),
            _ => ta.move_to(ta.right_of(ta.cur), shift),
        },

        _ => {}
    }
}
