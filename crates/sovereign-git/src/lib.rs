use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitSnapshot {
    pub repo_root: PathBuf,
    pub branch: BranchInfo,
    pub status: GitStatusSummary,
    pub captured_at_unix_ms: u128,
    pub remote_network_used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: Option<String>,
    pub is_detached: bool,
    pub head: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitStatusSummary {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicts: usize,
    pub entries: Vec<GitStatusEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub state: GitFileState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum GitFileState {
    Staged,
    Unstaged,
    Untracked,
    Conflict,
}

pub fn snapshot(path: impl AsRef<Path>) -> Result<GitSnapshot> {
    let repo_root = repo_root(path.as_ref())?;
    let branch = branch_info(&repo_root)?;
    let status = status_summary(&repo_root, branch.head.is_some())?;

    Ok(GitSnapshot {
        repo_root,
        branch,
        status,
        captured_at_unix_ms: now_unix_ms(),
        remote_network_used: false,
    })
}

fn repo_root(path: &Path) -> Result<PathBuf> {
    let root = run_git(path, ["rev-parse", "--show-toplevel"])?
        .trim()
        .to_string();
    PathBuf::from(root)
        .canonicalize()
        .context("failed to canonicalize git repo root")
}

fn branch_info(repo_root: &Path) -> Result<BranchInfo> {
    let name = run_git_optional(repo_root, ["symbolic-ref", "--quiet", "--short", "HEAD"])?
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let head = run_git_optional(repo_root, ["rev-parse", "--verify", "HEAD"])?
        .map(|head| head.trim().to_string())
        .filter(|head| !head.is_empty());

    Ok(BranchInfo {
        is_detached: name.is_none() && head.is_some(),
        name,
        head,
    })
}

fn status_summary(repo_root: &Path, has_head: bool) -> Result<GitStatusSummary> {
    let mut summary = GitStatusSummary::default();

    let staged = if has_head {
        run_git_lines(
            repo_root,
            [
                "diff-index",
                "--cached",
                "--name-only",
                "--diff-filter=ACDMRTUXB",
                "HEAD",
                "--",
            ],
        )?
    } else {
        run_git_lines(
            repo_root,
            [
                "diff",
                "--cached",
                "--name-only",
                "--diff-filter=ACDMRTUXB",
                "--no-ext-diff",
                "--no-renames",
                "--",
            ],
        )?
    };
    let unstaged = unstaged_paths_from_index_metadata(repo_root)?;
    let untracked = run_git_lines(repo_root, ["ls-files", "--others", "--exclude-standard"])?;
    let conflicts = unmerged_paths(repo_root)?;

    push_entries(&mut summary, GitFileState::Staged, staged);
    push_entries(&mut summary, GitFileState::Unstaged, unstaged);
    push_entries(&mut summary, GitFileState::Untracked, untracked);
    push_entries(&mut summary, GitFileState::Conflict, conflicts);

    summary.entries.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.state.cmp(&right.state))
    });
    Ok(summary)
}

fn unstaged_paths_from_index_metadata(repo_root: &Path) -> Result<BTreeSet<PathBuf>> {
    let output = run_git(repo_root, ["ls-files", "--debug"])?;
    let mut paths = BTreeSet::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_mtime: Option<(i64, u32)> = None;
    let mut current_size: Option<u64> = None;

    for line in output.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            maybe_push_unstaged_path(
                repo_root,
                &mut paths,
                current_path.take(),
                current_mtime.take(),
                current_size.take(),
            )?;
            current_path = Some(PathBuf::from(line));
            continue;
        }

        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("mtime: ") {
            current_mtime = parse_git_timestamp(value);
        } else if let Some(value) = trimmed.strip_prefix("size: ") {
            current_size = value
                .split_whitespace()
                .next()
                .and_then(|size| size.parse().ok());
        }
    }

    maybe_push_unstaged_path(
        repo_root,
        &mut paths,
        current_path,
        current_mtime,
        current_size,
    )?;

    Ok(paths)
}

fn maybe_push_unstaged_path(
    repo_root: &Path,
    paths: &mut BTreeSet<PathBuf>,
    path: Option<PathBuf>,
    index_mtime: Option<(i64, u32)>,
    index_size: Option<u64>,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };

    let full_path = repo_root.join(&path);
    let Ok(metadata) = fs::metadata(&full_path) else {
        paths.insert(path);
        return Ok(());
    };

    let worktree_size = metadata.len();
    let worktree_mtime = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs() as i64, duration.subsec_nanos()));

    if Some(worktree_size) != index_size || worktree_mtime != index_mtime {
        paths.insert(path);
    }

    Ok(())
}

fn parse_git_timestamp(value: &str) -> Option<(i64, u32)> {
    let (seconds, nanos) = value.split_once(':')?;
    Some((seconds.parse().ok()?, nanos.parse().ok()?))
}

fn unmerged_paths(repo_root: &Path) -> Result<BTreeSet<PathBuf>> {
    Ok(run_git(repo_root, ["ls-files", "--unmerged"])?
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, path)| PathBuf::from(path)))
        .collect())
}

fn push_entries(summary: &mut GitStatusSummary, state: GitFileState, paths: BTreeSet<PathBuf>) {
    for path in paths {
        match state {
            GitFileState::Staged => summary.staged += 1,
            GitFileState::Unstaged => summary.unstaged += 1,
            GitFileState::Untracked => summary.untracked += 1,
            GitFileState::Conflict => summary.conflicts += 1,
        }
        summary.entries.push(GitStatusEntry { path, state });
    }
}

fn run_git_lines<const N: usize>(repo_root: &Path, args: [&str; N]) -> Result<BTreeSet<PathBuf>> {
    Ok(run_git(repo_root, args)?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn run_git<const N: usize>(repo_root: &Path, args: [&str; N]) -> Result<String> {
    let output = git_command(repo_root, args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    String::from_utf8(output.stdout).context("git returned non-UTF-8 output")
}

fn run_git_optional<const N: usize>(repo_root: &Path, args: [&str; N]) -> Result<Option<String>> {
    let output = git_command(repo_root, args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .context("git returned non-UTF-8 output");
    }

    Ok(None)
}

fn git_command<const N: usize>(repo_root: &Path, args: [&str; N]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", git_null_config_path())
        .env("GIT_ATTR_NOSYSTEM", "1");
    command
}

fn git_null_config_path() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn snapshot_reports_branch_without_network() {
        let repo = TestRepo::new();
        repo.git(["checkout", "-b", "feature/local-git"]);

        let snapshot = snapshot(repo.path()).expect("snapshot");

        assert_eq!(snapshot.branch.name.as_deref(), Some("feature/local-git"));
        assert!(!snapshot.branch.is_detached);
        assert!(!snapshot.remote_network_used);
    }

    #[test]
    fn snapshot_counts_untracked_and_unstaged_files() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "hello");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write("tracked.txt", "changed");
        repo.write("new.txt", "new");

        let snapshot = snapshot(repo.path()).expect("snapshot");

        assert_eq!(snapshot.status.unstaged, 1);
        assert_eq!(snapshot.status.untracked, 1);
        assert_eq!(snapshot.status.staged, 0);
        assert!(
            snapshot
                .status
                .entries
                .iter()
                .any(|entry| entry.path == Path::new("tracked.txt")
                    && entry.state == GitFileState::Unstaged)
        );
        assert!(
            snapshot
                .status
                .entries
                .iter()
                .any(|entry| entry.path == Path::new("new.txt")
                    && entry.state == GitFileState::Untracked)
        );
    }

    #[test]
    fn snapshot_counts_staged_files() {
        let repo = TestRepo::new();
        repo.write("staged.txt", "hello");
        repo.git(["add", "staged.txt"]);

        let snapshot = snapshot(repo.path()).expect("snapshot");

        assert_eq!(snapshot.status.staged, 1);
        assert_eq!(snapshot.status.unstaged, 0);
        assert_eq!(snapshot.status.untracked, 0);
    }

    #[test]
    fn snapshot_does_not_execute_clean_filters() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "hello\n");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write(
            "filter.sh",
            "#!/bin/sh\necho filter-ran >> \"$PWD/filter.log\"\ncat\n",
        );
        repo.chmod_executable("filter.sh");
        repo.git(["config", "filter.spy.clean", "./filter.sh"]);
        repo.write(".gitattributes", "*.txt filter=spy\n");
        repo.git(["add", ".gitattributes"]);
        repo.git(["commit", "-m", "Add attributes"]);
        repo.remove("filter.log");
        repo.write("tracked.txt", "HELLO\n");

        let snapshot = snapshot(repo.path()).expect("snapshot");

        assert_eq!(snapshot.status.unstaged, 1);
        assert!(!repo.path().join("filter.log").exists());
        assert!(!snapshot.remote_network_used);
    }

    struct TestRepo {
        dir: TempDir,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp dir");
            let repo = Self { dir };
            repo.git(["init"]);
            repo.git(["config", "user.email", "test@example.com"]);
            repo.git(["config", "user.name", "Test User"]);
            repo.git(["config", "commit.gpgsign", "false"]);
            repo
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, relative: &str, contents: &str) {
            fs::write(self.path().join(relative), contents).expect("write file");
        }

        fn remove(&self, relative: &str) {
            let _ = fs::remove_file(self.path().join(relative));
        }

        #[cfg(unix)]
        fn chmod_executable(&self, relative: &str) {
            use std::os::unix::fs::PermissionsExt;

            let path = self.path().join(relative);
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("set permissions");
        }

        #[cfg(not(unix))]
        fn chmod_executable(&self, _relative: &str) {}

        fn git<const N: usize>(&self, args: [&str; N]) {
            let output = Command::new("git")
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", git_null_config_path())
                .env("GIT_ATTR_NOSYSTEM", "1")
                .current_dir(self.path())
                .output()
                .expect("run git");

            assert!(
                output.status.success(),
                "git failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
