use super::{Entry, HashMap, Item, Link, PathBuf, PrRef, State, Value, gh, json};

/// Pages of 100 PRs read.
pub(super) const PAGES: usize = 5;

pub(super) fn dir() -> PathBuf {
    crate::engine::data_dir().join("prs")
}

pub(super) fn item_json(item: &Item) -> Value {
    json!({"repo": item.pr.repo, "number": item.pr.number, "title": item.title, "state": item.state.label(), "created": item.created, "head": item.head})
}

pub(super) fn item_of(value: &Value) -> Option<Item> {
    let state = match value["state"].as_str()? {
        "draft" => State::Draft,
        "merged" => State::Merged,
        "closed" => State::Closed,
        _ => State::Open,
    };

    Some(Item {
        pr: PrRef::new(value["repo"].as_str()?, value["number"].as_u64()?),
        title: value["title"].as_str().unwrap_or_default().into(),
        state,
        created: value["created"].as_u64().unwrap_or(0),
        head: value["head"].as_str().unwrap_or_default().into(),
    })
}

/// PR → its thread: the remembered lists, then the threads' checkouts on the PR's branch.
pub(super) fn link(items: &[Item]) -> Vec<Option<Link>> {
    let threads: Vec<crate::engine::store::Thread> = crate::engine::store::all()
        .into_iter()
        .filter(|thread| thread.state != "archived" && thread.state != "deleted")
        .collect();
    let remembered: Value = std::fs::read(dir().join("threads.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}));
    let mut by_pr: HashMap<String, &crate::engine::store::Thread> = HashMap::new();

    for current_thread in &threads {
        for key_2 in remembered[&current_thread.id]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            by_pr.entry(key_2.to_string()).or_insert(current_thread);
        }
    }

    // (repo, branch) of every checkout: 1 git call per dir
    let mut repo_of: HashMap<String, Option<String>> = HashMap::new();
    let mut by_branch: HashMap<(String, String), &crate::engine::store::Thread> = HashMap::new();

    for candidate_thread in &threads {
        index_checkouts(&mut repo_of, &mut by_branch, candidate_thread);
    }

    items
        .iter()
        .map(|item| {
            let matched_thread = by_pr
                .get(&item.pr.key())
                .or_else(|| by_branch.get(&(item.pr.repo.clone(), item.head.clone())))?;

            Some(Link {
                id: matched_thread.id.clone(),
                title: matched_thread.title.clone(),
                dir: matched_thread.work_dir(),
            })
        })
        .collect()
}

pub(super) fn index_checkouts<'threads>(
    repo_of: &mut HashMap<String, Option<String>>,
    by_branch: &mut HashMap<(String, String), &'threads crate::engine::store::Thread>,
    thread: &'threads crate::engine::store::Thread,
) {
    for checkout in thread.all_checkouts() {
        let Some(branch_value) = checkout.branch.clone() else {
            continue;
        };

        let repo = repo_of
            .entry(checkout.dir.clone())
            .or_insert_with(|| gh::repo_of_dir(std::path::Path::new(&checkout.dir)))
            .clone();

        if let Some(repository) = repo {
            by_branch.entry((repository, branch_value)).or_insert(thread);
        }
    }
}

pub(super) fn load() -> Result<Vec<Entry>, String> {
    let items = gh::mine(PAGES)?;
    let _ = std::fs::create_dir_all(dir());
    let _ = crate::engine::write_atomic(
        &dir().join("mine.json"),
        json!(items.iter().map(item_json).collect::<Vec<_>>())
            .to_string()
            .as_bytes(),
    );
    let links = link(&items);

    Ok(items
        .into_iter()
        .zip(links)
        .map(|(item, thread)| Entry { item, thread })
        .collect())
}

pub(super) fn cached() -> Vec<Entry> {
    let value: Value = std::fs::read(dir().join("mine.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(item_of)
        .map(|item| Entry { item, thread: None })
        .collect()
}
