//! Push the marola umbrella's branches and tags from GitHub to its Codeberg mirror.
//! Run by hand from a full (not shallow) clone; see README.md next to Cargo.toml.

use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const DEFAULT_REPO: &str = "h0ffmann/marola";
const DEFAULT_REMOTE: &str = "codeberg";
const DEFAULT_SOURCE: &str = "origin";
const CODEBERG_HOST: &str = "codeberg.org";

const USAGE: &str = "\
codeberg-mirror: push the marola umbrella's branches and tags to Codeberg

usage:
  codeberg-mirror setup [--https] [options]   add or update the Codeberg git remote
  codeberg-mirror check [options]             check that Codeberg answers with your credentials
  codeberg-mirror push [--dry-run] [--prune] [options]
                                              fetch the source remote, push its branches and tags

options:
  --repo OWNER/NAME   Codeberg repository (default: h0ffmann/marola)
  --remote NAME       git remote for Codeberg (default: codeberg)
  --source NAME       git remote to mirror from (default: origin)
  --url URL           remote URL, instead of the one built from --repo
  -C DIR              run in DIR instead of the current directory

auth:
  SSH (default): your key must be on codeberg.org/user/settings/keys.
  HTTPS (setup --https): CODEBERG_TOKEN is read on every check/push and is never written to
  .git/config. CODEBERG_USER defaults to the owner in --repo.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Setup,
    Check,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Ssh,
    Https,
}

#[derive(Debug, PartialEq, Eq)]
struct Opts {
    cmd: Cmd,
    transport: Transport,
    repo: String,
    remote: String,
    source: String,
    url: Option<String>,
    dir: PathBuf,
    dry_run: bool,
    prune: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Run(Opts),
    Help,
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut args = args.into_iter();
    let cmd = match args.next().as_deref() {
        None | Some("-h" | "--help" | "help") => return Ok(Parsed::Help),
        Some("setup") => Cmd::Setup,
        Some("check") => Cmd::Check,
        Some("push") => Cmd::Push,
        Some(other) => return Err(format!("unknown command '{other}'")),
    };
    let mut o = Opts {
        cmd,
        transport: Transport::Ssh,
        repo: DEFAULT_REPO.into(),
        remote: DEFAULT_REMOTE.into(),
        source: DEFAULT_SOURCE.into(),
        url: None,
        dir: PathBuf::from("."),
        dry_run: false,
        prune: false,
    };
    while let Some(a) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match a.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "--https" if cmd == Cmd::Setup => o.transport = Transport::Https,
            "--dry-run" if cmd == Cmd::Push => o.dry_run = true,
            "--prune" if cmd == Cmd::Push => o.prune = true,
            "--repo" => o.repo = value("--repo")?,
            "--remote" => o.remote = value("--remote")?,
            "--source" => o.source = value("--source")?,
            "--url" => o.url = Some(value("--url")?),
            "-C" => o.dir = PathBuf::from(value("-C")?),
            other => return Err(format!("unknown option '{other}' for this command")),
        }
    }
    validate_repo(&o.repo)?;
    if o.remote == o.source {
        return Err(format!("--remote and --source are both '{}'", o.remote));
    }
    Ok(Parsed::Run(o))
}

fn validate_repo(repo: &str) -> Result<(), String> {
    let ok_part = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    match repo.split_once('/') {
        Some((owner, name)) if ok_part(owner) && ok_part(name) => Ok(()),
        _ => Err(format!("--repo '{repo}' is not OWNER/NAME")),
    }
}

fn remote_url(repo: &str, transport: Transport) -> String {
    match transport {
        Transport::Ssh => format!("git@{CODEBERG_HOST}:{repo}.git"),
        Transport::Https => format!("https://{CODEBERG_HOST}/{repo}.git"),
    }
}

/// Ref names (`refs/heads/x`, `refs/tags/v1`) from `git ls-remote` output, without the peeled
/// `^{}` lines annotated tags add.
fn ls_remote_refs(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.split_once('\t').map(|(_, r)| r))
        .filter(|r| !r.ends_with("^{}"))
        .map(String::from)
        .collect()
}

/// One refspec per ref instead of `refs/remotes/<source>/*`: git ignores a negative `^…/HEAD`
/// refspec on push, so the wildcard would create a branch named HEAD on Codeberg. `+` because a
/// mirror follows force-pushes. With `prune`, refs only on Codeberg are deleted.
fn push_refspecs(
    source: &str,
    source_refs: &[String],
    mirror_refs: &[String],
    prune: bool,
) -> Vec<String> {
    let mut specs: Vec<String> = source_refs
        .iter()
        .filter_map(|r| {
            if let Some(b) = r.strip_prefix("refs/heads/") {
                Some(format!("+refs/remotes/{source}/{b}:{r}"))
            } else if r.starts_with("refs/tags/") {
                Some(format!("+{r}:{r}"))
            } else {
                None
            }
        })
        .collect();
    if prune {
        specs.extend(
            mirror_refs
                .iter()
                .filter(|r| r.starts_with("refs/heads/") || r.starts_with("refs/tags/"))
                .filter(|r| !source_refs.contains(r))
                .map(|r| format!(":{r}")),
        );
    }
    specs
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Git config passed through GIT_CONFIG_COUNT/KEY_n/VALUE_n, so the token stays out of argv
/// (visible in `ps`) and out of .git/config. Appends after any pairs the caller already set.
fn auth_env(url: &str, user: &str, token: &str, existing_count: usize) -> Vec<(String, String)> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Vec::new();
    };
    let host = rest.split('/').next().unwrap_or(rest);
    let header = format!(
        "Authorization: Basic {}",
        base64(format!("{user}:{token}").as_bytes())
    );
    let n = existing_count;
    vec![
        ("GIT_CONFIG_COUNT".into(), (n + 1).to_string()),
        (
            format!("GIT_CONFIG_KEY_{n}"),
            format!("http.https://{host}/.extraHeader"),
        ),
        (format!("GIT_CONFIG_VALUE_{n}"), header),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
    ]
}

struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
}

impl Git {
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.dir).args(args);
        c.envs(self.env.iter().map(|(k, v)| (k, v)));
        c
    }

    fn output(&self, args: &[&str]) -> Result<String, String> {
        let out = self
            .cmd(args)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    fn run(&self, args: &[&str]) -> Result<(), String> {
        eprintln!("+ git {}", args.join(" "));
        let status = self
            .cmd(args)
            .status()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("git {} exited with {status}", args.join(" ")))
        }
    }

    fn remote_url(&self, name: &str) -> Option<String> {
        self.output(&["remote", "get-url", name]).ok()
    }
}

fn setup(o: &Opts) -> Result<(), String> {
    let git = Git {
        dir: o.dir.clone(),
        env: Vec::new(),
    };
    git.output(&["rev-parse", "--git-dir"])?;
    if git.remote_url(&o.source).is_none() {
        return Err(format!(
            "no '{}' remote here: run this in a clone of the umbrella",
            o.source
        ));
    }
    let url = o
        .url
        .clone()
        .unwrap_or_else(|| remote_url(&o.repo, o.transport));
    match git.remote_url(&o.remote) {
        Some(old) if old == url => println!("remote '{}' already points at {url}", o.remote),
        Some(old) => {
            git.run(&["remote", "set-url", &o.remote, &url])?;
            println!("remote '{}': {old} -> {url}", o.remote);
        }
        None => {
            git.run(&["remote", "add", &o.remote, &url])?;
            println!("remote '{}' added: {url}", o.remote);
        }
    }
    if url.starts_with("https://") {
        println!("HTTPS: export CODEBERG_TOKEN (a Codeberg access token with repository write) before check/push");
    }
    println!("next: codeberg-mirror check, then codeberg-mirror push --dry-run");
    Ok(())
}

fn authed_git(o: &Opts) -> Result<(Git, String), String> {
    let plain = Git {
        dir: o.dir.clone(),
        env: Vec::new(),
    };
    let url = plain.remote_url(&o.remote).ok_or_else(|| {
        format!(
            "no '{}' remote: run 'codeberg-mirror setup' first",
            o.remote
        )
    })?;
    let mut env = Vec::new();
    if url.starts_with("https://") {
        let token = env::var("CODEBERG_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .ok_or("remote is HTTPS and CODEBERG_TOKEN is not set")?;
        let user = env::var("CODEBERG_USER")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| o.repo.split('/').next().unwrap_or_default().to_string());
        let count = env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        env = auth_env(&url, &user, &token, count);
    }
    Ok((
        Git {
            dir: o.dir.clone(),
            env,
        },
        url,
    ))
}

fn check(o: &Opts) -> Result<(), String> {
    let (git, url) = authed_git(o)?;
    let heads = git.output(&["ls-remote", "--heads", &o.remote])?;
    let n = heads.lines().filter(|l| !l.is_empty()).count();
    println!("{url}: reachable, {n} branch(es)");
    Ok(())
}

fn push(o: &Opts) -> Result<(), String> {
    let (git, url) = authed_git(o)?;
    if git.output(&["rev-parse", "--is-shallow-repository"])? == "true" {
        return Err(format!(
            "this clone is shallow; Codeberg would reject the push. Run: git fetch --unshallow {}",
            o.source
        ));
    }
    git.run(&["fetch", "--prune", "--tags", &o.source])?;
    let source_refs =
        ls_remote_refs(&git.output(&["ls-remote", "--heads", "--tags", &o.source])?);
    let mirror_refs = if o.prune {
        ls_remote_refs(&git.output(&["ls-remote", "--heads", "--tags", &o.remote])?)
    } else {
        Vec::new()
    };
    let specs = push_refspecs(&o.source, &source_refs, &mirror_refs, o.prune);
    if specs.is_empty() {
        return Err(format!("'{}' has no branches or tags to mirror", o.source));
    }
    let mut args = vec!["push"];
    if o.dry_run {
        args.push("--dry-run");
    }
    args.push(&o.remote);
    args.extend(specs.iter().map(String::as_str));
    git.run(&args)?;
    println!(
        "{}{url} mirrors '{}'",
        if o.dry_run { "dry run: " } else { "" },
        o.source
    );
    Ok(())
}

fn main() -> ExitCode {
    let result = match parse_args(env::args().skip(1)) {
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Run(o)) => match o.cmd {
            Cmd::Setup => setup(&o),
            Cmd::Check => check(&o),
            Cmd::Push => push(&o),
        },
        Err(e) => Err(format!("{e}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("codeberg-mirror: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn run_opts(s: &str) -> Opts {
        match parse_args(args(s)).unwrap() {
            Parsed::Run(o) => o,
            Parsed::Help => panic!("help"),
        }
    }

    #[test]
    fn parses_defaults_and_flags() {
        let o = run_opts("push --dry-run --prune");
        assert_eq!((o.cmd, o.dry_run, o.prune), (Cmd::Push, true, true));
        assert_eq!(
            (o.repo.as_str(), o.remote.as_str()),
            (DEFAULT_REPO, DEFAULT_REMOTE)
        );
        assert_eq!(run_opts("setup --https").transport, Transport::Https);
        assert_eq!(run_opts("check --repo a/b").repo, "a/b");
        assert_eq!(parse_args(args("")).unwrap(), Parsed::Help);
        assert_eq!(parse_args(args("push --help")).unwrap(), Parsed::Help);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_args(args("pull")).is_err());
        assert!(parse_args(args("setup --dry-run")).is_err());
        assert!(parse_args(args("push --https")).is_err());
        assert!(parse_args(args("push --repo")).is_err());
        assert!(parse_args(args("push --repo marola")).is_err());
        assert!(parse_args(args("push --repo a/../b")).is_err());
        assert!(parse_args(args("push --remote origin")).is_err());
    }

    #[test]
    fn builds_urls() {
        assert_eq!(
            remote_url("h0ffmann/marola", Transport::Ssh),
            "git@codeberg.org:h0ffmann/marola.git"
        );
        assert_eq!(
            remote_url("h0ffmann/marola", Transport::Https),
            "https://codeberg.org/h0ffmann/marola.git"
        );
    }

    #[test]
    fn parses_ls_remote() {
        let out = "a1\trefs/heads/main\nb2\trefs/tags/v1\nc3\trefs/tags/v1^{}\n";
        assert_eq!(ls_remote_refs(out), ["refs/heads/main", "refs/tags/v1"]);
    }

    #[test]
    fn refspecs_force_and_prune() {
        let src = vec![
            "refs/heads/main".to_string(),
            "refs/tags/v1".into(),
            "refs/pull/1/head".into(),
        ];
        let mirror = vec![
            "refs/heads/main".to_string(),
            "refs/heads/old".into(),
            "refs/tags/v0".into(),
        ];
        assert_eq!(
            push_refspecs("origin", &src, &mirror, false),
            [
                "+refs/remotes/origin/main:refs/heads/main",
                "+refs/tags/v1:refs/tags/v1"
            ]
        );
        assert_eq!(
            push_refspecs("origin", &src, &mirror, true)[2..],
            [":refs/heads/old", ":refs/tags/v0"]
        );
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn auth_env_only_for_https_and_appends() {
        assert!(auth_env("git@codeberg.org:a/b.git", "u", "t", 0).is_empty());
        let e = auth_env("https://codeberg.org/a/b.git", "u", "t", 2);
        assert_eq!(e[0], ("GIT_CONFIG_COUNT".into(), "3".into()));
        assert_eq!(
            e[1],
            (
                "GIT_CONFIG_KEY_2".into(),
                "http.https://codeberg.org/.extraHeader".into()
            )
        );
        assert_eq!(e[2].1, format!("Authorization: Basic {}", base64(b"u:t")));
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn refs(bare: &Path) -> Vec<String> {
        let mut v: Vec<String> = sh(bare, &["for-each-ref", "--format=%(refname) %(objectname)"])
            .lines()
            .map(String::from)
            .collect();
        v.sort();
        v
    }

    /// GitHub and Codeberg stood in for by two local bare repos.
    #[test]
    fn setup_then_push_mirrors_branches_and_tags() {
        let root = env::temp_dir().join(format!("codeberg-mirror-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (gh, cb, work) = (root.join("gh.git"), root.join("cb.git"), root.join("work"));
        std::fs::create_dir_all(&root).unwrap();
        sh(
            &root,
            &["init", "-q", "--bare", "-b", "main", gh.to_str().unwrap()],
        );
        sh(
            &root,
            &["init", "-q", "--bare", "-b", "main", cb.to_str().unwrap()],
        );
        sh(
            &root,
            &["clone", "-q", gh.to_str().unwrap(), work.to_str().unwrap()],
        );
        sh(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        sh(&work, &["tag", "v0.1.0"]);
        sh(&work, &["branch", "feature"]);
        sh(
            &work,
            &["push", "-q", "origin", "main", "feature", "v0.1.0"],
        );
        sh(&work, &["remote", "set-head", "origin", "main"]);
        // A branch only on Codeberg, which --prune must remove.
        sh(&work, &["push", "-q", cb.to_str().unwrap(), "main:stale"]);

        let url = format!("file://{}", cb.display());
        let o = |s: &str| {
            let mut o = run_opts(s);
            o.dir = work.clone();
            o.url = Some(url.clone());
            o
        };
        setup(&o("setup")).unwrap();
        assert_eq!(sh(&work, &["remote", "get-url", "codeberg"]).trim(), url);
        setup(&o("setup")).unwrap();

        push(&o("push --dry-run")).unwrap();
        assert_eq!(refs(&cb).len(), 1);

        push(&o("push --prune")).unwrap();
        assert_eq!(refs(&cb), refs(&gh), "cb vs gh");
        assert!(!refs(&cb).iter().any(|r| r.starts_with("refs/heads/HEAD ")));

        // A force-push on GitHub reaches the mirror too.
        sh(
            &work,
            &[
                "commit",
                "-q",
                "--amend",
                "--allow-empty",
                "-m",
                "one, amended",
            ],
        );
        sh(&work, &["push", "-q", "--force", "origin", "main"]);
        push(&o("push")).unwrap();
        assert_eq!(refs(&cb), refs(&gh));
        check(&o("check")).unwrap();

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn push_without_remote_says_run_setup() {
        let o = run_opts("push --remote nowhere-xyz");
        assert!(push(&o).unwrap_err().contains("setup"));
    }
}
