# Changelog

## [Unreleased]

- `--completions <shell>` prints a shell completion script (bash, zsh, fish,
  powershell, elvish).

## [0.2.0] - 2026-06-03

- Optional config file (`$XDG_CONFIG_HOME/cruft/config.toml` or `--config`) for
  custom anchored rules: match a directory name when a sibling file exists, or
  anywhere for a tool-specific cache name.

## 0.1.0

First release.

- Recursively scan a directory for reclaimable build and cache directories.
- Size as on-disk usage (du-style, hard links counted once), or logical bytes
  with `--apparent`. MODIFIED is the newest mtime in the subtree.
- Filter by `--min-size`, `--older-than`, and `--only <types>` (combined as AND).
- `--sort size|modified|path`, `--reverse`, and `--limit`.
- `--total-only` for a single total, no table.
- `--json` output.
- Delete to the trash (recoverable): `--delete` with a confirmation prompt,
  `-y` to skip it, `-i` to choose per directory, `--dry-run` to preview. Each
  directory is re-checked just before removal and skipped if it changed.
- Matched types: node_modules, target (Cargo.toml or pom.xml), .next, .nuxt,
  .turbo, .svelte-kit, .parcel-cache, .gradle, .tox, .venv/venv, __pycache__,
  .pytest_cache, .mypy_cache, .ruff_cache. Ambiguous names are matched only when
  the relevant project file sits next to them.
