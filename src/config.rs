use std::env;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::scan::{builtin_rules, labels, Anchor, Rule};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    name: String,
    dir: String,
    #[serde(default)]
    anchors: Vec<String>,
    #[serde(default)]
    anywhere: bool,
}

// $XDG_CONFIG_HOME/cruft/config.toml, falling back to ~/.config/cruft/config.toml
fn default_path() -> Option<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("cruft").join("config.toml"))
}

// Builtin rules plus whatever the config adds. An explicit --config that's
// missing is an error; a missing default config just means no custom rules.
pub fn load_rules(explicit: Option<&Path>) -> Result<Vec<Rule>, String> {
    let mut rules = builtin_rules();

    let path = match explicit {
        Some(p) => {
            if !p.is_file() {
                return Err(format!("config not found: {}", p.display()));
            }
            p.to_path_buf()
        }
        None => match default_path() {
            Some(p) if p.is_file() => p,
            _ => return Ok(rules),
        },
    };

    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let parsed: ConfigFile = toml::from_str(&text)
        .map_err(|e| format!("{}: {}", path.display(), first_line(&e.to_string())))?;

    let custom = validate(parsed.rules, &rules)?;
    rules.extend(custom);
    Ok(rules)
}

fn validate(raw: Vec<RawRule>, builtin: &[Rule]) -> Result<Vec<Rule>, String> {
    let builtin_labels = labels(builtin);
    let builtin_dirs: Vec<&str> = builtin.iter().map(|r| r.dir.as_str()).collect();

    let mut custom = Vec::new();
    let mut seen_dirs: Vec<String> = Vec::new();

    for r in raw {
        match (r.anchors.is_empty(), r.anywhere) {
            (true, false) => {
                return Err(format!(
                    "rule '{}': needs either anchors or anywhere = true",
                    r.name
                ))
            }
            (false, true) => {
                return Err(format!(
                    "rule '{}': anchors and anywhere are mutually exclusive",
                    r.name
                ))
            }
            _ => {}
        }
        if builtin_labels.iter().any(|l| l == &r.name) {
            return Err(format!(
                "rule '{}': name clashes with a builtin type",
                r.name
            ));
        }
        if builtin_dirs.contains(&r.dir.as_str()) {
            return Err(format!(
                "rule '{}': dir '{}' clashes with a builtin rule",
                r.name, r.dir
            ));
        }
        if seen_dirs.contains(&r.dir) {
            return Err(format!("dir '{}' is used by more than one rule", r.dir));
        }
        seen_dirs.push(r.dir.clone());

        let anchor = if r.anywhere {
            Anchor::Anywhere
        } else {
            Anchor::Sibling(r.anchors)
        };
        custom.push(Rule {
            label: r.name,
            dir: r.dir,
            anchor,
        });
    }
    Ok(custom)
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn load_str(toml_text: &str) -> Result<Vec<Rule>, String> {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text).unwrap();
        load_rules(Some(&path))
    }

    fn custom_only(rules: Vec<Rule>) -> Vec<Rule> {
        let n = builtin_rules().len();
        rules.into_iter().skip(n).collect()
    }

    #[test]
    fn valid_config_adds_a_rule() {
        let rules = load_str(
            r#"
            [[rules]]
            name = "cocoapods"
            dir = "Pods"
            anchors = ["Podfile"]
        "#,
        )
        .unwrap();
        let custom = custom_only(rules);
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].label, "cocoapods");
        assert_eq!(custom[0].dir, "Pods");
    }

    #[test]
    fn missing_explicit_config_is_an_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.toml");
        assert!(load_rules(Some(&path)).is_err());
    }

    #[test]
    fn neither_anchor_nor_anywhere_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "x"
            dir = "X"
        "#,
        )
        .unwrap_err();
        assert!(err.contains("anchors or anywhere"));
    }

    #[test]
    fn both_anchor_and_anywhere_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "x"
            dir = "X"
            anchors = ["a"]
            anywhere = true
        "#,
        )
        .unwrap_err();
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn name_clashing_with_builtin_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "target"
            dir = "MyTarget"
            anywhere = true
        "#,
        )
        .unwrap_err();
        assert!(err.contains("clashes with a builtin"));
    }

    #[test]
    fn dir_clashing_with_builtin_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "mine"
            dir = "node_modules"
            anywhere = true
        "#,
        )
        .unwrap_err();
        assert!(err.contains("clashes with a builtin"));
    }

    #[test]
    fn duplicate_custom_dir_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "a"
            dir = "Same"
            anywhere = true

            [[rules]]
            name = "b"
            dir = "Same"
            anywhere = true
        "#,
        )
        .unwrap_err();
        assert!(err.contains("more than one rule"));
    }

    #[test]
    fn broken_toml_fails() {
        assert!(load_str("this is = = not toml").is_err());
    }

    #[test]
    fn wrong_field_type_fails() {
        let err = load_str(
            r#"
            [[rules]]
            name = "x"
            dir = "X"
            anchors = "Podfile"
        "#,
        )
        .unwrap_err();
        assert!(!err.is_empty());
    }
}
