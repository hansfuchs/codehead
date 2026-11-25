use super::{BTreeMap, Form, config};

pub(super) fn aliases(form: &Form) -> usize {
    form.fields.len().saturating_sub(form.plain()) / 2
}

/// 1st navigation item of each section: general, the SECTIONS, the aliases.
pub(super) fn starts(form: &Form) -> Vec<usize> {
    form.sections
        .iter()
        .map(|section| section.0)
        .chain(form.aliases_on.then(|| form.plain()))
        .collect()
}

/// The last navigation item: the add row (0 aliases: the last field).
pub(super) fn add_item(form: &Form) -> usize {
    if form.aliases_on {
        form.plain() + self::aliases(form)
    } else {
        form.plain().saturating_sub(1)
    }
}

/// The text field keys go to: a plain field, or a field of the open alias box.
pub(super) fn field(form: &Form) -> Option<usize> {
    if form.focus < form.plain() {
        Some(form.focus)
    } else {
        form.editing
    }
}

/// The alias box of field I (≥ 6).
pub(super) fn alias_of(i: usize, form: &Form) -> usize {
    (i - form.plain()) / 2
}

/// Plain field I back to its default: blank, applied.
pub(super) fn reset(i: usize, form: &mut Form) {
    if i >= form.plain() || form.fields[i].text().is_empty() {
        return;
    }

    form.fields[i].set_text("");
    form.commit();

    if !form.error {
        form.status = format!("{}: default", form.specs[i].key);
    }
}

pub(super) fn values(form: &Form) -> Result<config::File, String> {
    if form.fields.len() < form.plain() {
        return Err("reload config before save".into());
    }

    let typed = |i: usize| {
        let text = form.fields[i].text().trim().to_string();

        (!text.is_empty()).then_some(text)
    };

    let mut aliases = BTreeMap::new();

    for pair in form.fields[form.plain()..].as_chunks::<2>().0 {
        let name = pair[0].text().trim().to_string();
        let target = pair[1].text().trim().to_string();

        // a new box, 0 text yet: not an alias
        if name.is_empty() && target.is_empty() {
            continue;
        }

        config::validate_alias(&name, &target)?;

        if aliases.insert(name.clone(), target).is_some() {
            return Err(format!("duplicate alias: {name}"));
        }
    }

    let mut file = form.saved.clone();

    if form.aliases_on {
        file.aliases = aliases;
    }

    for (i, specification) in form.specs.iter().enumerate() {
        let value = typed(i);

        if let Some(text) = &value {
            config::validate(specification.key, text)?;
        }

        file.set(specification.key, value);
    }

    Ok(file)
}

pub(super) fn dirty(form: &Form) -> bool {
    self::values(form).ok().is_none_or(|value| value != form.saved)
}

pub(super) fn delete_alias(alias: usize, form: &mut Form) {
    let at = form.plain() + alias * 2;

    if at + 2 > form.fields.len() {
        return;
    }

    form.fields.drain(at..at + 2);
    form.editing = None;
    form.focus = form.focus.min(self::add_item(form));
    form.commit();
}

/// Esc: the focused field's text as it was at entry (an open alias box: both its fields), 0
/// commit. Esc never applies.
pub(super) fn revert(form: &mut Form) {
    if let Some(i) = form.editing {
        if let Some((at, name, target)) = form.pair_entry.take()
            && at == form.plain() + self::alias_of(i, form) * 2
            && (form.fields[at].text() != name || form.fields[at + 1].text() != target)
        {
            form.fields[at].set_text(&name);
            form.fields[at + 1].set_text(&target);
            form.status = "alias edit undone".into();
        }

        form.editing = None;
        return;
    }

    if let Some((i, text)) = form.entry.clone()
        && Some(i) == self::field(form)
        && form.fields[i].text() != text
    {
        form.fields[i].set_text(&text);
        form.fields[i].move_start();
        form.status = format!("{}: edit undone", form.specs.get(i).map_or("field", |spec| spec.key));
    }
}
