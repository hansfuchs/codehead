//! Names a mechanical rename makes: a `_value` or `_2` suffix, a word twice in a row
//! (`PopupStateState`, `sweep_name_name`), a key code in a fn name (`key_keycode_char`), a
//! trailing `_`. A leading `_` (`_guard`: held, never read) is fine. Name it after what it holds or does.

use syn::visit::Visit;
use syn::{Field, ImplItemFn, ItemEnum, ItemFn, ItemStruct, PatIdent, TraitItemFn};

use crate::{Finding, Source};

pub(crate) fn check(source: &Source) -> Vec<Finding> {
    let Ok(file) = syn::parse_file(&source.text) else {
        return Vec::new();
    };

    let mut visitor = Visitor { names: Vec::new() };

    visitor.visit_file(&file);

    visitor
        .names
        .into_iter()
        .filter_map(|(line, name)| problem(&name).map(|why| source.finding(line, format!("`{name}`: {why}"))))
        .collect()
}

fn problem(name: &str) -> Option<&'static str> {
    let trimmed = name.trim_start_matches("r#");
    let words = words(trimmed);
    let last = words.last().map_or("", String::as_str);

    if trimmed.len() > 1 && (trimmed.ends_with('_') || trimmed.contains("__")) {
        return Some("trailing or double `_`: a generated name");
    }

    if words.len() > 1 && (last == "value" || last.chars().all(|digit| digit.is_ascii_digit())) {
        return Some("`_value` / number suffix: a generated name, name it after what it holds");
    }

    if words.windows(2).any(|pair| pair[0] == pair[1]) {
        return Some("a word twice in a row: a generated name");
    }

    if words.iter().any(|word| word == "keycode") {
        return Some("a key code in a name: name it after the action");
    }

    None
}

/// Lowercase words of a `snake_case` or `CamelCase` name.
fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut after_lower = false;

    for character in name.chars() {
        let boundary = character == '_' || (character.is_uppercase() && after_lower);

        after_lower = character.is_lowercase() || character.is_ascii_digit();

        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }

        if character != '_' {
            current.extend(character.to_lowercase());
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

struct Visitor {
    names: Vec<(usize, String)>,
}

impl Visitor {
    fn add(&mut self, ident: &syn::Ident) {
        self.names.push((ident.span().start().line, ident.to_string()));
    }
}

impl<'ast> Visit<'ast> for Visitor {
    fn visit_pat_ident(&mut self, item: &'ast PatIdent) {
        self.add(&item.ident);
        syn::visit::visit_pat_ident(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.add(&item.sig.ident);
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.add(&item.sig.ident);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        self.add(&item.sig.ident);
        syn::visit::visit_trait_item_fn(self, item);
    }

    fn visit_field(&mut self, item: &'ast Field) {
        if let Some(ident) = &item.ident {
            self.add(ident);
        }

        syn::visit::visit_field(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        self.add(&item.ident);
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        self.add(&item.ident);
        syn::visit::visit_item_enum(self, item);
    }
}
