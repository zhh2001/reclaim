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
PATH                       TYPE               SIZE  MODIFIED
web/node_modules           node_modules    1.2 GiB  3 months ago
svc/target                 target        430.1 MiB  2 days ago
api/.venv                  venv           88.4 MiB  5 months ago

Reclaimable total: 1.7 GiB
```

Use `--json` for machine-readable output.

## Filtering

Narrow the results (and, with `--delete`, the set that gets removed):

- `--min-size <SIZE>` — keep entries at least this big. Plain number is bytes;
  `K`/`M`/`G`/`T` suffixes are 1024-based (`500K`, `1.5G`). Uses the current
  size mode, so it follows `--apparent`.
- `--older-than <DURATION>` — keep entries untouched for at least this long,
  based on the newest mtime in the tree. Units are `h`, `d`, `w` (`12h`, `30d`,
  `2w`); minutes/months aren't accepted to avoid the `m` ambiguity.
- `--only <TYPES>` — comma-separated, from: `node_modules`, `target`,
  `__pycache__`, `venv`, `pytest_cache`, `mypy_cache`.

Combining them is an AND: an entry must pass every filter to be kept. If nothing
matches, reclaim says so and exits without deleting.

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

SIZE is on-disk usage (block allocation, hard links counted once), matching
`du`. Pass `--apparent` to sum logical file sizes instead, counting every hard
link. Off Unix only the apparent figure is available. MODIFIED is the newest
mtime found anywhere in the directory, not the directory's own timestamp.

## Build

```
cargo build --release
```
