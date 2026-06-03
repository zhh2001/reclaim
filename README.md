# cruft

[![crates.io](https://img.shields.io/crates/v/cruft.svg)](https://crates.io/crates/cruft)
[![CI](https://github.com/zhh2001/cruft/actions/workflows/ci.yml/badge.svg)](https://github.com/zhh2001/cruft/actions/workflows/ci.yml)

A command-line tool that finds build and cache directories you can safely delete
(`node_modules`, Rust `target`, Python caches, and so on) and reports how much
space they take. It can also move them to the trash.

## Install

Prebuilt binaries for Linux, macOS, and Windows are attached to each
[GitHub release](https://github.com/zhh2001/cruft/releases) — download the
archive for your platform and put `cruft` on your PATH.

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall cruft
```

If `cargo binstall` falls back to building from source, you're probably hitting GitHub's unauthenticated API rate limit — common behind a shared or corporate IP. Set `GITHUB_TOKEN` to any personal access token (no scopes needed for a public repo) and it'll fetch the prebuilt binary.

From crates.io (builds from source):

```sh
cargo install cruft
```

Or build the checkout yourself:

```sh
git clone https://github.com/zhh2001/cruft
cd cruft
cargo build --release
# binary at target/release/cruft
```

## Usage

Scan the current directory:

```txt
$ cruft
PATH              TYPE               SIZE  MODIFIED
web/node_modules  node_modules    1.1 MiB  just now
svc/target        target        392.0 KiB  just now
api/.venv         venv           92.0 KiB  6 months ago
py/__pycache__    __pycache__     8.0 KiB  just now

Reclaimable total: 1.6 MiB
```

More examples:

```sh
cruft ~/code                        # scan a specific path
cruft --min-size 100M               # filters combine as AND
cruft --older-than 30d
cruft --only node_modules,target
cruft --sort modified               # oldest first
cruft --limit 10                    # ten largest
cruft --total-only                  # just the total, no table
```

Delete: look first, then do it. Matches go to the trash; without `-y` you're
asked to confirm.

```sh
cruft --delete --dry-run
cruft --delete                      # prompts before moving anything
cruft --delete -y                   # no prompt
cruft --delete -i                   # ask per directory (y/n/q)
```

`--delete` honours the filters and `--limit`, so it only removes what the same
command would have listed. Each directory is re-checked right before it's moved;
if it changed since the scan it's reported and skipped.

## Shell completions

`--completions <shell>` prints a completion script to stdout (bash, zsh, fish,
powershell, elvish). Redirect it to wherever your shell looks:

```sh
cruft --completions bash > ~/.local/share/bash-completion/completions/cruft
cruft --completions zsh  > ~/.zfunc/_cruft
cruft --completions fish > ~/.config/fish/completions/cruft.fish
```

## Man page

`--man` prints a man page in roff to stdout:

```sh
cruft --man > ~/.local/share/man/man1/cruft.1
```

## What it matches

Directories whose name is a tool-specific cache match anywhere:
`__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`.

Everything else has an ambiguous name, so it only counts when the matching
project file sits next to it:

- `node_modules` — `package.json`
- `target` — `Cargo.toml` (Rust) or `pom.xml` (Maven)
- `.next`, `.nuxt`, `.turbo`, `.svelte-kit`, `.parcel-cache` — `package.json`
- `.gradle` — a `build.gradle`/`settings.gradle` file (`.kts` too)
- `.tox` — `tox.ini`
- `.venv` / `venv` — a `pyvenv.cfg` inside

`dist` and `build` are left out on purpose; the names are too ambiguous to match
without risking a real source directory. Once a directory matches, its contents
aren't scanned again, so nested matches are counted once.

## Custom rules

Add your own types in `$XDG_CONFIG_HOME/cruft/config.toml` (or
`~/.config/cruft/config.toml`), or point at a file with `--config`. Each rule
needs a `name` (shown in the TYPE column, usable with `--only`) and a `dir`
(exact directory name), plus either `anchors` (sibling files, any one is enough)
or `anywhere = true` for an unambiguous cache name:

```toml
[[rules]]
name = "cocoapods"
dir = "Pods"
anchors = ["Podfile"]

[[rules]]
name = "mytool-cache"
dir = ".mytool"
anywhere = true
```

Custom rules add to the builtin ones; a rule's name or dir can't collide with a
builtin. With no config file, behaviour is unchanged.

## Safety

Deletion moves directories to the trash (the XDG trash on Linux), so a mistake
is recoverable. Nothing is removed without `-y` or an interactive `y`. If a
directory can't be trashed, it's reported and the rest still proceed.

## Accuracy

Sizes are on-disk usage, the same as `du`: block allocation, with hard links
counted once. Use `--apparent` for logical file sizes instead. MODIFIED is the
newest mtime found anywhere in the directory.

One caveat: pnpm stores packages in a global store and hard-links them into each
`node_modules`, so deleting one project's `node_modules` may free less than the
reported size, since the linked files still live in the store.

## License

MIT OR Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
