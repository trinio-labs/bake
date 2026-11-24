// End-to-end tests for sharing the local cache across git worktrees.
//
// These drive `bake::run_bake` (the CLI entry point) or the `bake` binary so the
// wiring in lib.rs that picks the cache directory is exercised, not just
// `resolve_cache_path`.

use std::path::{Path, PathBuf};

use bake::{Args, project::GIT_LOCAL_ENV_VARS, run_bake};
use clap::Parser;
use tempfile::TempDir;
use test_case::test_case;

/// Runs git in `dir`, panicking on failure. Identity, signing and repository
/// variables are pinned so the test is independent of the caller's environment.
fn git(dir: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    for var in GIT_LOCAL_ENV_VARS {
        cmd.env_remove(var);
    }
    let status = cmd
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A main worktree at `<base>/main` holding a committed bake project, and the
/// path where a linked worktree will be added (`<base>/wt`).
struct Repo {
    _tmp: TempDir,
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
}

impl Repo {
    fn add_worktree(&self) {
        git(
            &self.main,
            &["worktree", "add", "-q", self.wt.to_str().unwrap()],
        );
    }
}

/// Creates a committed project at `<main>/<project_rel>` whose `app:gen` recipe
/// writes a value that differs on every execution to `app/out.txt`, so a cache
/// hit is observable as an identical value.
fn setup_repo(project_rel: &str) -> Repo {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let main = base.join("main");
    let project = main.join(project_rel);
    std::fs::create_dir_all(project.join("app")).unwrap();
    std::fs::write(
        project.join("bake.yml"),
        "name: wtc\nconfig:\n  update:\n    enabled: false\n",
    )
    .unwrap();
    // `od` on /dev/urandom works under any POSIX `sh` (dash has no $RANDOM).
    std::fs::write(
        project.join("app/cookbook.yml"),
        "name: app\nrecipes:\n  gen:\n    cache:\n      inputs:\n        - cookbook.yml\n      outputs:\n        - out.txt\n    run: od -An -N8 -tu8 /dev/urandom > out.txt\n",
    )
    .unwrap();
    std::fs::write(main.join(".gitignore"), ".bake/\nout.txt\n").unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "init"]);
    Repo {
        _tmp: tmp,
        wt: base.join("wt"),
        base,
        main,
    }
}

async fn bake(project: &Path, extra: &[&str]) {
    let mut argv = vec!["bake", "-p", project.to_str().unwrap()];
    argv.extend_from_slice(extra);
    run_bake(Args::try_parse_from(argv).unwrap()).await.unwrap();
}

/// EX-003: a recipe baked in the main worktree is a cache hit in a linked worktree.
#[tokio::test]
async fn test_linked_worktree_hits_main_worktree_cache() {
    let repo = setup_repo("");
    bake(&repo.main, &["app:gen"]).await;
    let main_out = std::fs::read_to_string(repo.main.join("app/out.txt")).unwrap();
    assert!(!main_out.trim().is_empty(), "recipe wrote no value");

    repo.add_worktree();
    bake(&repo.wt, &["app:gen"]).await;
    let wt_out = std::fs::read_to_string(repo.wt.join("app/out.txt")).unwrap();

    assert_eq!(main_out, wt_out, "linked worktree re-ran the recipe");
    assert!(!repo.wt.join(".bake/cache").exists());
}

/// EX-009: `--clean` in a linked worktree removes the main worktree's cache.
#[tokio::test]
async fn test_clean_in_linked_worktree_removes_main_cache() {
    let repo = setup_repo("");
    bake(&repo.main, &["app:gen"]).await;
    let main_cache = repo.main.join(".bake/cache");
    assert!(main_cache.exists());

    repo.add_worktree();
    // Pre-create a local cache dir so a clean that targets it would be observable too.
    std::fs::create_dir_all(repo.wt.join(".bake/cache")).unwrap();
    bake(&repo.wt, &["--clean"]).await;

    assert!(!main_cache.exists(), "main worktree cache was not cleaned");
    assert!(repo.wt.join(".bake/cache").exists());
}

/// Repository-local git variables inherited by bake (e.g. when run from a git
/// hook) do not change where the cache lives. The variable is set only on the
/// `bake` child process; `{base}` in its value is the test's base directory.
#[test_case("sub/proj", false, "GIT_DIR", "{base}/main/.git" ; "GIT_DIR in main worktree subproject")]
#[test_case("", true, "GIT_WORK_TREE", "{base}/wt/app" ; "GIT_WORK_TREE in linked worktree")]
#[test_case("", true, "GIT_COMMON_DIR", "{base}/other/.git" ; "GIT_COMMON_DIR in linked worktree")]
#[test_case("", true, "GIT_CONFIG_PARAMETERS", "'core.worktree'='{base}/other'" ; "GIT_CONFIG_PARAMETERS core.worktree in linked worktree")]
#[test_case("", true, "GIT_CONFIG_PARAMETERS", "'core.bare'='maybe'" ; "GIT_CONFIG_PARAMETERS invalid core.bare in linked worktree")]
fn test_inherited_git_location_vars_do_not_move_cache(
    project_rel: &str,
    in_linked_worktree: bool,
    var: &str,
    value: &str,
) {
    let repo = setup_repo(project_rel);
    let other = repo.base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q"]);
    let checkout = if in_linked_worktree {
        repo.add_worktree();
        &repo.wt
    } else {
        &repo.main
    };

    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_bake"));
    for v in GIT_LOCAL_ENV_VARS {
        cmd.env_remove(v);
    }
    let output = cmd
        .args([
            "-p",
            checkout.join(project_rel).to_str().unwrap(),
            "app:gen",
        ])
        .env(var, value.replace("{base}", repo.base.to_str().unwrap()))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "bake failed: {output:?}");

    let expected = repo.main.join(project_rel).join(".bake/cache");
    assert!(expected.exists(), "cache not at {}", expected.display());
    for wrong in [
        repo.main.join(".bake/cache"),
        repo.wt.join(".bake/cache"),
        other.join(".bake/cache"),
    ] {
        if wrong != expected {
            assert!(!wrong.exists(), "cache created at {}", wrong.display());
        }
    }
}
