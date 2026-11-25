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
        if let Some(rows) = cur.get(&key) {
            return Some(rows.clone());
        }

        let rows = prev.remove(&key)?;

        cur.insert(key, rows.clone());
        Some(rows)
    });

    let rows = hit.unwrap_or_else(|| {
        let rows = Rc::new(make());

        MEMO.with_borrow_mut(|(cur, _)| cur.insert(key, rows.clone()));
        rows
    });

    rows.as_ref().clone()
}

fn key(what: &str, width: usize, prefix: &[Span<'static>], input: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut hasher = DefaultHasher::new();

    (
        what,
        width,
        prefix,
        crate::ui::theme::pal(),
        crate::ui::markdown::marks_hash(),
    )
        .hash(&mut hasher);
    input(&mut hasher);
    hasher.finish()
}

pub(crate) fn markdown(text: &str, width: usize, prefix: &[Span<'static>]) -> Vec<Row> {
    get(key("md", width, prefix, |hasher| text.hash(hasher)), || {
        crate::ui::markdown::markdown(text, width, prefix)
    })
}

pub(crate) fn call(call: &super::Call, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let memo_key = key("call", width, margin, |hasher| {
        (&call.id, &call.name, call.input.to_string(), &call.output).hash(hasher);
    });

    get(memo_key, || super::call_rows(call, width, margin))
}

/// End of a draw: bodies this draw did not use go.
pub(crate) fn sweep() {
    MEMO.with_borrow_mut(|(cur, prev)| *prev = std::mem::take(cur));
}
