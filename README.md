# reclaim

Scans a directory tree for build and cache directories that are safe to delete
and tells you how much space they're using. By default it only reports;
`--delete` moves the matches to the trash after you confirm.

## Usage

```
reclaim [PATH]
```

`PATH` defaults to the current directory. The scan recurses, including into
directories that are usually gitignored (`node_modules`, `target`), since those
are the whole point.

```
$ reclaim ~/code
PATH                       TYPE          SIZE  MODIFIED
web/node_modules           node_modules  1.2 GiB  3 months ago
svc/target                 target        430.1 MiB  2 days ago
api/.venv                  venv          88.4 MiB  5 months ago

Reclaimable total: 1.7 GiB
```

Use `--json` for machine-readable output.

## Deleting

```
reclaim --delete [PATH]
```

This scans, prints the same table, then asks before doing anything. Matches are
moved to the trash (XDG trash on Linux), never permanently removed, so a mistake
is recoverable.

- `-y`, `--yes` — skip the prompt
- `--dry-run` — print what would be trashed and stop; if combined with `--yes`,
  dry-run wins and nothing is deleted

If a directory can't be trashed (permissions, a filesystem without trash
support) it's reported and the rest still proceed. `--delete` can't be combined
with `--json`.

## What it detects

- `node_modules`, when there's a `package.json` next to it
- `target`, when there's a `Cargo.toml` next to it
- `__pycache__`, anywhere
- `.venv` / `venv`, when they contain a `pyvenv.cfg`
- `.pytest_cache`, `.mypy_cache`, anywhere

Once a directory matches it isn't scanned further, so a match nested inside
another match is counted once, as part of the outer one.

Sizes are the sum of file lengths (apparent size), not on-disk block usage.

## Build

```
cargo build --release
```
