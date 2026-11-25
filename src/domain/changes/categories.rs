use super::Cat;

/// Category of PATH by its name and dirs (heuristics; an agent ranking can replace them).
pub(crate) fn categorize(path: &str) -> Cat {
    let lowered = path.to_ascii_lowercase();
    let name = lowered.rsplit('/').next().unwrap_or(&lowered);
    let dirs: Vec<&str> = lowered.split('/').collect();
    let dirs = &dirs[..dirs.len().saturating_sub(1)];
    let ext = name.rsplit_once('.').map_or("", |(_, extension)| extension);

    if is_generated(name, dirs, ext) {
        return Cat::Generated;
    }

    if is_test(name, dirs) {
        return Cat::Tests;
    }

    if is_document(name, dirs, ext) {
        return Cat::Docs;
    }

    if is_schema(name, dirs, ext) {
        return Cat::Schema;
    }

    if is_configuration(name, dirs, ext) {
        return Cat::Config;
    }

    if is_interface(dirs, ext) {
        return Cat::Ui;
    }

    Cat::Logic
}

fn is_generated(name: &str, dirs: &[&str], ext: &str) -> bool {
    let lock = [
        "cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "poetry.lock",
        "go.sum",
        "gemfile.lock",
        "composer.lock",
        "uv.lock",
        "flake.lock",
    ];

    lock.contains(&name)
        || ext == "snap"
        || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.contains(".generated.")
        || name.ends_with(".pb.go")
        || dirs.iter().any(|directory| {
            [
                "__snapshots__",
                "dist",
                "generated",
                "__generated__",
                "vendor",
                "node_modules",
            ]
            .contains(directory)
        })
}
fn is_test(name: &str, dirs: &[&str]) -> bool {
    dirs.iter().any(|dir| {
        [
            "test",
            "tests",
            "spec",
            "specs",
            "__tests__",
            "e2e",
            "fixtures",
            "testdata",
        ]
        .contains(dir)
    }) || name.contains(".test.")
        || name.contains(".spec.")
        || name.contains("_test.")
        || name.starts_with("test_")
        || name.ends_with("_spec.rb")
}
fn is_document(name: &str, dirs: &[&str], ext: &str) -> bool {
    ["md", "mdx", "rst", "txt", "adoc"].contains(&ext)
        || dirs.contains(&"docs")
        || name.starts_with("license")
        || name.starts_with("changelog")
}
fn is_schema(name: &str, dirs: &[&str], ext: &str) -> bool {
    ["sql", "proto", "graphql", "gql", "prisma"].contains(&ext)
        || dirs
            .iter()
            .any(|dir| dir.contains("migration") || *dir == "schema" || *dir == "schemas")
        || name.contains("schema")
        || name.contains("openapi")
        || name.contains("swagger")
}
fn is_configuration(name: &str, dirs: &[&str], ext: &str) -> bool {
    [
        "toml", "yaml", "yml", "json", "jsonc", "ini", "cfg", "lock", "env", "example",
    ]
    .contains(&ext)
        || ["dockerfile", "makefile", "justfile", ".gitignore", ".editorconfig"].contains(&name)
        || name.contains(".config.")
        || dirs
            .first()
            .is_some_and(|top| *top == ".github" || *top == "config" || *top == ".vscode")
}
fn is_interface(dirs: &[&str], ext: &str) -> bool {
    ["css", "scss", "sass", "less", "html", "svelte", "vue", "astro", "svg"].contains(&ext)
        || dirs.iter().any(|dir| {
            [
                "components",
                "ui",
                "views",
                "pages",
                "styles",
                "templates",
                "public",
                "assets",
            ]
            .contains(dir)
        })
}
