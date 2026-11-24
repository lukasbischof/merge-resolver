# merge-resolver

A three-pane terminal merge tool for `git mergetool`, modelled on IntelliJ's merge dialog and driven by vim keys.

```
 Local                  │ Result                 │ Incoming
»  14  lat_dms = ...    │●  14  "#{latitude}..." │«  14  lat_dir = ...
```

The left pane shows your local version, the right pane the incoming one, and the middle pane the editable result.

## Build

Requires Rust 1.88 or newer (`rustup update stable`).

```sh
cargo build --release
```

The binary is written to `target/release/merge-resolver`. It has no runtime dependencies, so you can copy it anywhere on your `PATH`.

## Install

To build and copy it to `~/.cargo/bin` in one step:

```sh
cargo install --path .
```

Add to `~/.gitconfig`:

```ini
[merge]
    tool = merge-resolver
[mergetool "merge-resolver"]
    cmd = merge-resolver "$BASE" "$LOCAL" "$REMOTE" "$MERGED"
    trustExitCode = true
```

Then run `git mergetool` during a conflicted merge, rebase or cherry-pick.

## Keys

The conflict under the cursor is highlighted in all three panes and is the target of the conflict keys, whichever pane you are in.

Local changes are purple and incoming changes blue. The result pane tints the lines a change would replace: red while both sides can still land there, otherwise in the colour of the one side that can.

| Key | Action |
|---|---|
| `n` / `N` | Next / previous unresolved conflict |
| `]c` / `[c` | Next / previous change, resolved or not |
| `L` / `R` | Accept left / right; `LR` or `RL` takes both in that order |
| `XL` / `XR` | Ignore left / right |
| `XX` | Ignore both and keep the result as it is |
| `u` / `^r` | Undo / redo |
| `^h` / `^l` | Pane to the left / right (also `^w h`, `^w l`, `^w w`) |
| `<space>` | Menu with the remaining commands; `<space>?` lists all keys |

Everything else is vim. Motions, counts, visual mode and yanking work in every pane; editing works in the result pane only. Search is literal with smartcase, and repeats with `gn` / `gN` because `n` / `N` jump between conflicts.

## Saving and quitting

| Command | Effect |
|---|---|
| `:w` | Write the result |
| `:wq`, `ZZ` | Write and quit; refused while conflicts are unresolved |
| `:wq!` | Write and quit regardless |
| `:q`, `ZQ` | Quit, keeping whatever was last written |
| `:cq` | Quit and report failure to git |

The exit code is 0 only if the result was written at least once, so git treats a quit without writing as an unresolved file.

## Behaviour

- Non-conflicting changes are applied when the tool starts. Pass `--no-auto-apply` to start from the base and apply them yourself.
- Changes to adjacent lines count as one conflict, as they do in git.
- A conflict you fix by hand is marked resolved with `XX`.
- Line endings and a missing final newline are preserved.
- Colours are tuned for dark terminals, with a 256-colour fallback when `COLORTERM` does not announce truecolor.

## Development

```sh
cargo test
cargo clippy --all-targets
```
