//! Size limits: lines per file, lines per function (blank and comment lines not counted).

use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{ImplItemFn, ItemFn, TraitItemFn};

use crate::{Finding, MAX_FILE_LINES, MAX_FN_LINES, Source};

pub(crate) fn check(source: &Source) -> Vec<Finding> {
    let lines = source.lines();
    let mut findings = Vec::new();

    if lines.len() > MAX_FILE_LINES {
        let message = format!(
            "file has {} lines, max. {MAX_FILE_LINES}: split it by concern",
            lines.len()
        );

        findings.push(source.finding(1, message));
    }

    let Ok(file) = syn::parse_file(&source.text) else {
        findings.push(source.finding(1, "file does not parse"));
        return findings;
    };

    let mut visitor = Visitor {
        lines: &lines,
        long: Vec::new(),
    };

    visitor.visit_file(&file);

    for (line, name, count) in visitor.long {
        let message = format!("fn `{name}` has {count} lines, max. {MAX_FN_LINES}: split it");

        findings.push(source.finding(line, message));
    }

    findings
}

struct Visitor<'lines> {
    lines: &'lines [&'lines str],
    long: Vec<(usize, String, usize)>,
}

impl Visitor<'_> {
    fn measure(&mut self, name: String, span: proc_macro2::Span) {
        let start = span.start().line;
        let end = span.end().line;
        let count = self
            .lines
            .get(start.saturating_sub(1)..end)
            .unwrap_or_default()
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .count();

        if count > MAX_FN_LINES {
            self.long.push((start, name, count));
        }
    }
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let span = item.sig.span().join(item.block.span()).unwrap_or_else(|| item.span());

        self.measure(item.sig.ident.to_string(), span);
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        let span = item.sig.span().join(item.block.span()).unwrap_or_else(|| item.span());

        self.measure(item.sig.ident.to_string(), span);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        if let Some(block) = &item.default {
            let span = item.sig.span().join(block.span()).unwrap_or_else(|| item.span());

            self.measure(item.sig.ident.to_string(), span);
        }

        syn::visit::visit_trait_item_fn(self, item);
    }
}
