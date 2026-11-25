use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ratatui::text::Span;

use crate::ui::markdown::Row;

type Map = HashMap<u64, Rc<Vec<Row>>>;

thread_local! {
    /// (used in this draw, used in the draw before).
    static MEMO: RefCell<(Map, Map)> = RefCell::default();
}

fn get(key: u64, make: impl FnOnce() -> Vec<Row>) -> Vec<Row> {
    let hit = MEMO.with_borrow_mut(|(cur, prev)| {
        if let Some(result) = cur.get(&key) {
            return Some(result.clone());
        }

        let result_value = prev.remove(&key)?;

        cur.insert(key, result_value.clone());
        Some(result_value)
    });

    let rows = hit.unwrap_or_else(|| {
        let result_2 = Rc::new(make());

        MEMO.with_borrow_mut(|(cur, _)| cur.insert(key, result_2.clone()));
        result_2
    });

    rows.as_ref().clone()
}

fn key(what: &str, width: usize, prefix: &[Span<'static>], input: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut height = DefaultHasher::new();

    (
        what,
        width,
        prefix,
        crate::ui::theme::pal(),
        crate::ui::markdown::marks_hash(),
    )
        .hash(&mut height);
    input(&mut height);
    height.finish()
}

pub(crate) fn markdown(text: &str, width: usize, prefix: &[Span<'static>]) -> Vec<Row> {
    get(key("md", width, prefix, |height| text.hash(height)), || {
        crate::ui::markdown::markdown(text, width, prefix)
    })
}

pub(crate) fn call(character: &super::Call, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let key_value = key("call", width, margin, |height| {
        (
            &character.id,
            &character.name,
            character.input.to_string(),
            &character.output,
        )
            .hash(height);
    });

    get(key_value, || super::call_rows(character, width, margin))
}

/// End of a draw: bodies this draw did not use go.
pub(crate) fn sweep() {
    MEMO.with_borrow_mut(|(cur, prev)| *prev = std::mem::take(cur));
}
