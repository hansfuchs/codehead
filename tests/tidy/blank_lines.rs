//! Blank lines between the blocks of a function body.
//!
//! 1. after a multi-line block statement (`if`, `for`, `while`, `loop`, `match`, block, closure body)
//! 2. before a multi-line block statement
//! 3. between a group of `let` bindings and the statement after it
//! 4. before a comment line that opens a group
//! 5. before the tail expression of a function with more than 5 statements
//! 6. tests: between act and assert (an `assert*!` next to a statement that is not one)
//!
//! rustfmt keeps the rest: max. 1 blank line in a row, 0 blank lines after `{` and before `}`.

use std::path::Path;

use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Block, Expr, ImplItemFn, ItemFn, Stmt};

use crate::{Finding, Source};

struct Gap {
    after_line: usize,
    reason: &'static str,
}

pub(crate) fn check(source: &Source) -> Vec<Finding> {
    gaps(source)
        .into_iter()
        .map(|gap| source.finding(gap.after_line + 1, gap.reason))
        .collect()
}

/// Inserts the missing blank lines. True: the file changed.
pub(crate) fn fix(path: &Path, source: &Source) -> bool {
    let gaps = gaps(source);

    if gaps.is_empty() {
        return false;
    }

    let mut lines: Vec<&str> = source.text.lines().collect();

    for gap in gaps.iter().rev() {
        lines.insert(gap.after_line, "");
    }

    let mut text = lines.join("\n");

    text.push('\n');

    std::fs::write(path, text).is_ok()
}

fn gaps(source: &Source) -> Vec<Gap> {
    let Ok(file) = syn::parse_file(&source.text) else {
        return Vec::new();
    };

    let lines = source.lines();
    let mut visitor = Visitor {
        lines: &lines,
        gaps: Vec::new(),
        in_test: false,
    };

    visitor.visit_file(&file);

    let mut gaps = visitor.gaps;

    gaps.sort_by_key(|gap| gap.after_line);
    gaps.dedup_by_key(|gap| gap.after_line);

    gaps
}

struct Visitor<'lines> {
    lines: &'lines [&'lines str],
    gaps: Vec<Gap>,
    in_test: bool,
}

impl Visitor<'_> {
    fn pair(&mut self, first: &Stmt, second: &Stmt) {
        let end = first.span().end().line;
        let start = second.span().start().line;
        let next = self.lines.get(end).map_or("", |line| line.trim());

        if start <= end || next.is_empty() {
            return;
        }

        let reason = if is_block(first) {
            "blank line missing after a multi-line block"
        } else if is_block(second) {
            "blank line missing before a multi-line block"
        } else if is_let(first) && !is_let(second) {
            "blank line missing after the let group"
        } else if next.starts_with("//") {
            "blank line missing before a comment that opens a group"
        } else if self.in_test && is_assert(first) != is_assert(second) {
            "blank line missing between act and assert"
        } else {
            return;
        };

        self.gaps.push(Gap {
            after_line: end,
            reason,
        });
    }

    fn tail(&mut self, body: &Block) {
        let [.., before, Stmt::Expr(_, None)] = body.stmts.as_slice() else {
            return;
        };

        let end = before.span().end().line;
        let next = self.lines.get(end).map_or("", |line| line.trim());

        if body.stmts.len() > 5 && !next.is_empty() {
            self.gaps.push(Gap {
                after_line: end,
                reason: "blank line missing before the tail expression",
            });
        }
    }
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let outer = self.in_test;

        self.in_test = item.attrs.iter().any(|attr| attr.path().is_ident("test"));
        self.tail(&item.block);
        syn::visit::visit_item_fn(self, item);
        self.in_test = outer;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.tail(&item.block);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        for (first, second) in block.stmts.iter().zip(block.stmts.iter().skip(1)) {
            self.pair(first, second);
        }

        syn::visit::visit_block(self, block);
    }
}

fn is_multi_line(stmt: &Stmt) -> bool {
    let span = stmt.span();

    span.end().line > span.start().line
}

fn is_block(stmt: &Stmt) -> bool {
    let block_like = match stmt {
        Stmt::Expr(expr, _) => is_block_expr(expr) || has_block_closure(expr),
        Stmt::Local(local) => local
            .init
            .as_ref()
            .is_some_and(|init| init.diverge.is_some() || is_block_expr(&init.expr) || has_block_closure(&init.expr)),
        Stmt::Item(_) => true,
        Stmt::Macro(_) => false,
    };

    block_like && is_multi_line(stmt)
}

fn is_block_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::If(_)
            | Expr::ForLoop(_)
            | Expr::While(_)
            | Expr::Loop(_)
            | Expr::Match(_)
            | Expr::Block(_)
            | Expr::Unsafe(_)
            | Expr::TryBlock(_)
            | Expr::Const(_)
    )
}

fn has_block_closure(expr: &Expr) -> bool {
    match expr {
        Expr::Closure(closure) => matches!(*closure.body, Expr::Block(_)),
        Expr::MethodCall(call) => has_block_closure(&call.receiver) || call.args.iter().any(has_block_closure),
        Expr::Call(call) => call.args.iter().any(has_block_closure),
        Expr::Assign(assign) => is_block_expr(&assign.right) || has_block_closure(&assign.right),
        Expr::Await(inner) => has_block_closure(&inner.base),
        Expr::Try(inner) => has_block_closure(&inner.expr),
        _ => false,
    }
}

fn is_let(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Local(_))
}

fn is_assert(stmt: &Stmt) -> bool {
    let path = match stmt {
        Stmt::Macro(item) => &item.mac.path,
        Stmt::Expr(Expr::Macro(item), _) => &item.mac.path,
        _ => return false,
    };

    path.segments.last().is_some_and(|segment| {
        let name = segment.ident.to_string();

        name.starts_with("assert") || name.starts_with("debug_assert")
    })
}
