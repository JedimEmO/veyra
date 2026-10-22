//! The layer order of the sim's adapter modules, held as `just server-check`
//! holds the crate order: a module may name a module of its own layer or a
//! lower one by a `crate::` path, never a higher one. The rules those
//! adapters sit on are `veyra-rules`, which holds the same table over its own
//! modules (`crates/veyra-rules/tests/layers.rs`); a module that moved there
//! entirely is re-exported from `lib.rs` and keeps its layer in this table, so
//! an adapter that climbs into it is still reported.
//!
//! The modules used to be one strongly connected component (every rule
//! file named half of the others), which kept the game half from being
//! split into crates that compile side by side. The layers below are the
//! cut lines of that split; the check reads each module's non-test source
//! at test time, so a new module has to be given a layer here before it
//! builds green, and an edge that climbs fails with the file and line.
//!
//! A schedule edge is not a `crate::` path any more: the order requests are
//! answered in is `order::RequestSet`, in one place.

#![allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a failed read fails, which is the point"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Lowest first. Modules of one layer may name each other.
const LAYERS: &[(&str, &[&str])] = &[
    (
        "state: the standing with the factions, the deeds, the order, the view state the screens read",
        &[
            "household",
            "standing",
            "factions",
            "deeds",
            "order",
            "view",
        ],
    ),
    (
        "combat and progression",
        &[
            "combat",
            "combat_loadout",
            "combat_tables",
            "combat_tiers",
            "combat_ai",
            "combat_curves",
            "ability",
            "progression",
        ],
    ),
    ("gear", &["items", "fitting", "wear", "equip"]),
    ("crafting", &["crafting", "blueprint", "factory", "rework"]),
    ("land", &["deposits", "harvest", "ride"]),
    (
        "life",
        &[
            "encounters",
            "loot",
            "postings",
            "procurement",
            "quests",
            "fall",
            "chart",
        ],
    ),
];

fn layer_of() -> BTreeMap<&'static str, usize> {
    LAYERS
        .iter()
        .enumerate()
        .flat_map(|(layer, (_, modules))| modules.iter().map(move |m| (*m, layer)))
        .collect()
}

/// `text` without the items that follow a `#[cfg(test)]`, and without
/// comments, so a doc link up the layers or a test helper is not an edge.
fn non_test(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "#[cfg(test)]".len()..];
        // The item is a `mod x;` line, or a braced block (or a braced item).
        let semi = after.find(';');
        let brace = after.find('{');
        match (semi, brace) {
            (Some(s), Some(b)) if s < b => skip(&mut out, &mut rest, after, s + 1),
            (Some(s), None) => skip(&mut out, &mut rest, after, s + 1),
            (_, Some(b)) => {
                let mut depth = 0_usize;
                let mut end = after.len();
                for (i, c) in after[b..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = b + i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                skip(&mut out, &mut rest, after, end);
            }
            (None, None) => skip(&mut out, &mut rest, after, after.len()),
        }
    }
    out.push_str(rest);
    // A comment line is blanked, not dropped, so line numbers stay the file's.
    out.lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Drops `after[..to]` from the text still to be read, keeping its line
/// breaks in `out` so a reported line is the source's own.
fn skip<'a>(out: &mut String, rest: &mut &'a str, after: &'a str, to: usize) {
    out.extend(after[..to].chars().filter(|c| *c == '\n'));
    *rest = &after[to..];
}

/// The source files of `module` that are not test files.
fn files_of(src: &Path, module: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let single = src.join(format!("{module}.rs"));
    if single.is_file() {
        files.push(single);
    }
    let dir = src.join(module);
    if dir.is_dir() {
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if stem != "tests" {
                        stack.push(path);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") && !stem.ends_with("tests") {
                    files.push(path);
                }
            }
        }
    }
    files
}

/// The names `lib.rs` re-exports from a module of this crate, with the
/// module: `crate::Reach` is `crate::progression::Reach`.
fn root_reexports(src: &Path, layers: &BTreeMap<&str, usize>) -> BTreeMap<String, String> {
    let lib = fs::read_to_string(src.join("lib.rs")).unwrap();
    let mut names = BTreeMap::new();
    let mut rest = lib.as_str();
    while let Some(at) = rest.find("pub use ") {
        rest = &rest[at + "pub use ".len()..];
        let Some(end) = rest.find(';') else { break };
        let item = &rest[..end];
        if let Some((module, group)) = item.split_once("::")
            && layers.contains_key(module.trim())
        {
            for name in group
                .trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
                .split(',')
            {
                let name = name.trim();
                if !name.is_empty() {
                    names.insert(name.to_owned(), module.trim().to_owned());
                }
            }
        }
    }
    names
}

/// The first identifier of `path`.
fn head(path: &str) -> &str {
    let end = path
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(path.len());
    &path[..end]
}

/// The heads of the top-level entries of a brace group whose text starts
/// just after the `{`: `a::B, c::{D, E}, F` gives `a`, `c`, `F`. The group
/// may span lines and nest.
fn group_heads(text: &str) -> Vec<&str> {
    let mut heads = Vec::new();
    let mut depth = 1_usize;
    let mut start = 0_usize;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth > 1 => depth -= 1,
            ',' if depth > 1 => {}
            ',' | '}' => {
                heads.push(head(text[start..i].trim()));
                start = i + 1;
                if c == '}' {
                    break;
                }
            }
            _ => {}
        }
    }
    heads.retain(|h| !h.is_empty());
    heads
}

/// Every module named by a `crate::` path in `code`, with the line. A
/// `use crate::{..}` group is read across lines and nested braces, because
/// rustfmt wraps a long import list into exactly that.
fn named(code: &str, reexports: &BTreeMap<String, String>) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(rel) = code[from..].find("crate::") {
        let at = from + rel;
        from = at + "crate::".len();
        // `strata_crate::` is not this crate.
        let before_ok = code[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if !before_ok {
            continue;
        }
        let line = code[..at].matches('\n').count() + 1;
        let rest = &code[from..];
        let firsts: Vec<&str> = if let Some(group) = rest.strip_prefix('{') {
            group_heads(group)
        } else {
            vec![head(rest)]
        };
        for first in firsts {
            let module = reexports.get(first).map_or(first, String::as_str);
            found.push((line, module.to_owned()));
        }
    }
    found
}

#[test]
fn modules_name_only_their_own_layer_or_lower() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let layers = layer_of();

    // Every module is in the table and every entry is a module.
    let mut on_disk: Vec<String> = fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter_map(|p| {
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            (stem != "lib" && (p.is_dir() || p.extension().is_some_and(|e| e == "rs")))
                .then_some(stem)
        })
        .collect();
    on_disk.sort();
    on_disk.dedup();
    let unplaced: Vec<&String> = on_disk
        .iter()
        .filter(|m| !layers.contains_key(m.as_str()))
        .collect();
    assert!(
        unplaced.is_empty(),
        "modules with no layer in tests/layers.rs: {unplaced:?}"
    );
    // A module that moved whole is a `pub use veyra_rules::{..}` in lib.rs, not a file.
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    let gone: Vec<&&str> = layers
        .keys()
        .filter(|m| !on_disk.contains(&(**m).to_owned()))
        .filter(|m| {
            !lib.contains("pub use veyra_rules::{")
                || !lib.contains(&format!(" {m},")) && !lib.contains(&format!(" {m}\n"))
        })
        .collect();
    assert!(
        gone.is_empty(),
        "layers name modules that are gone: {gone:?}"
    );

    let reexports = root_reexports(&src, &layers);
    assert!(
        reexports.contains_key("Reach"),
        "lib.rs no longer re-exports `Reach` from a module; update the root re-export reader"
    );

    let mut climbs = Vec::new();
    let mut checked = 0_usize;
    for (module, layer) in &layers {
        for file in files_of(&src, module) {
            let code = non_test(&fs::read_to_string(&file).unwrap());
            for (line, target) in named(&code, &reexports) {
                let Some(up) = layers.get(target.as_str()) else {
                    continue;
                };
                if target == *module {
                    continue;
                }
                checked += 1;
                if up > layer {
                    climbs.push(format!(
                        "{}:{line}: `{module}` (layer {layer}) names `{target}` (layer {up})",
                        file.strip_prefix(&src).unwrap().display()
                    ));
                }
            }
        }
    }
    assert!(
        checked > 50,
        "the reader found {checked} edges; it is not reading"
    );
    assert!(
        climbs.is_empty(),
        "a module names a higher layer:\n{}",
        climbs.join("\n")
    );
}

#[test]
fn the_layer_names_read_in_order() {
    // Guards the table against a reorder that would silently flip the rule.
    let names: Vec<&str> = LAYERS
        .iter()
        .map(|(name, _)| name.split(':').next().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "state",
            "combat and progression",
            "gear",
            "crafting",
            "land",
            "life"
        ]
    );
}

#[test]
fn a_wrapped_or_nested_group_is_read() {
    let reexports = BTreeMap::new();
    let wrapped = "use crate::{\n    combat::Combatant,\n    encounters::Fights,\n};";
    let heads: Vec<_> = named(wrapped, &reexports)
        .into_iter()
        .map(|e| e.1)
        .collect();
    assert_eq!(heads, ["combat", "encounters"]);
    let nested = "use crate::{combat::{A, B}, quests::X, items};";
    let heads: Vec<_> = named(nested, &reexports).into_iter().map(|e| e.1).collect();
    assert_eq!(heads, ["combat", "quests", "items"]);
    let plain = "let x = crate::deposits::Survey::new();";
    assert_eq!(named(plain, &reexports), [(1, "deposits".to_owned())]);
}
