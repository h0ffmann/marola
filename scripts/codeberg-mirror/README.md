# codeberg-mirror

Pushes the umbrella's branches and tags from GitHub to its mirror at
<https://codeberg.org/h0ffmann/marola>. It runs by hand on your machine: no workflow calls it yet.
It mirrors this repo only, not the submodules; their gitlinks and `.gitmodules` still point at
GitHub.

Rust, std only, so the build fetches no crates. Needs `cargo` (rustup) and `git` 2.31 or later.

## Use

From a full clone of the umbrella (a shallow clone is refused, because Codeberg rejects a push
whose history it cannot complete):

```bash
git clone https://github.com/marola-dev/marola && cd marola
just codeberg-mirror setup            # adds the `codeberg` remote, git@codeberg.org:h0ffmann/marola.git
just codeberg-mirror check            # ls-remote with your credentials
just codeberg-mirror push --dry-run   # what would change
just codeberg-mirror push             # do it
```

Without `just`: `cargo run --release --manifest-path scripts/codeberg-mirror/Cargo.toml -- <command>`.

`push` fetches `origin` first, then pushes each of origin's branches (from `refs/remotes/origin/*`)
and tags. Each ref is pushed with `+`, so a force-push on GitHub is copied to Codeberg. A branch
or tag that exists only on Codeberg stays there unless you add `--prune`, which deletes it.
Local branches that are not on origin are never pushed.

## Auth

- **SSH** (default): add your public key at <https://codeberg.org/user/settings/keys>.
- **HTTPS with a token**: `just codeberg-mirror setup --https`, then create a token at
  <https://codeberg.org/user/settings/applications> with the `write:repository` scope and
  `export CODEBERG_TOKEN=…` before `check` or `push`. The token is passed to git through
  `GIT_CONFIG_*` environment variables for that one command, so it is never in `.git/config` or
  in the process arguments. `CODEBERG_USER` defaults to the owner in `--repo`.

The repository must exist on Codeberg before the first push.

## Options

| Option | Default | |
|---|---|---|
| `--repo OWNER/NAME` | `h0ffmann/marola` | Codeberg repository |
| `--remote NAME` | `codeberg` | git remote for Codeberg |
| `--source NAME` | `origin` | git remote to mirror from |
| `--url URL` | built from `--repo` | any remote URL, for example another forge or a local bare repo |
| `-C DIR` | `.` | run in another checkout |

## Tests

```bash
cargo test --manifest-path scripts/codeberg-mirror/Cargo.toml
```

The end-to-end test uses two local bare repos for GitHub and Codeberg. It covers `setup`, `--dry-run`,
`--prune`, a force-push, and that origin's `HEAD` does not become a branch on the mirror.
