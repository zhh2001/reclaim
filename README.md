# reclaim

Scans a directory tree for build and cache directories that are safe to delete
and tells you how much space they're using. This version only reports — it never
deletes anything.

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
