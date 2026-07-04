use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitSnapshot {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub repo_root: PathBuf,
    pub branch: BranchInfo,
    pub status: GitStatusSummary,
    pub captured_at_unix_ms: u128,
    pub remote_network_used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffSummary {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub repo_root: PathBuf,
    pub staged: GitDiffScopeSummary,
    pub unstaged: GitDiffScopeSummary,
    pub captured_at_unix_ms: u128,
    pub patch_contents_included: bool,
    pub remote_network_used: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffScopeSummary {
    pub files: usize,
    pub insertions: u64,
    pub deletions: u64,
    pub entries: Vec<GitDiffEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitDiffEntry {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
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
    #[serde(serialize_with = "serialize_path_lossy")]
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

pub fn diff_summary(path: impl AsRef<Path>) -> Result<GitDiffSummary> {
    let repo_root = repo_root(path.as_ref())?;
    let diff_config_overrides = safe_diff_config_overrides(&repo_root)?;
    let staged = diff_scope_summary(
        &repo_root,
        &diff_config_overrides,
        [
            "diff",
            "--cached",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--",
        ],
    )?;
    let unstaged = diff_scope_summary(
        &repo_root,
        &diff_config_overrides,
        [
            "diff",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--",
        ],
    )?;

    Ok(GitDiffSummary {
        repo_root,
        staged,
        unstaged,
        captured_at_unix_ms: now_unix_ms(),
        patch_contents_included: false,
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

fn diff_scope_summary<const N: usize>(
    repo_root: &Path,
    config_overrides: &[(String, String)],
    args: [&str; N],
) -> Result<GitDiffScopeSummary> {
    let output = run_git_bytes_with_config(repo_root, config_overrides, args)?;
    let mut summary = GitDiffScopeSummary::default();

    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let entry = parse_numstat_record(record)?;
        if !entry.binary {
            summary.insertions += entry.insertions.unwrap_or_default();
            summary.deletions += entry.deletions.unwrap_or_default();
        }
        summary.files += 1;
        summary.entries.push(entry);
    }

    summary
        .entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(summary)
}

fn safe_diff_config_overrides(repo_root: &Path) -> Result<Vec<(String, String)>> {
    let mut overrides = Vec::new();

    for driver in repo_filter_drivers(repo_root)? {
        overrides.push((format!("filter.{driver}.clean"), "cat".to_string()));
        overrides.push((format!("filter.{driver}.smudge"), "cat".to_string()));
        overrides.push((format!("filter.{driver}.process"), String::new()));
        overrides.push((format!("filter.{driver}.required"), "false".to_string()));
    }

    Ok(overrides)
}

fn repo_filter_drivers(repo_root: &Path) -> Result<BTreeSet<String>> {
    let Some(output) = run_git_optional(
        repo_root,
        [
            "config",
            "--includes",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.",
        ],
    )?
    else {
        return Ok(BTreeSet::new());
    };

    Ok(output
        .lines()
        .filter_map(|key| key.strip_prefix("filter."))
        .filter_map(|key| key.rsplit_once('.').map(|(driver, _setting)| driver))
        .filter(|driver| !driver.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn parse_numstat_record(record: &[u8]) -> Result<GitDiffEntry> {
    let mut parts = record.splitn(3, |byte| *byte == b'\t');
    let insertions = parts.next().with_context(|| {
        format!(
            "missing insertion count in numstat record '{}'",
            display_git_record(record)
        )
    })?;
    let deletions = parts.next().with_context(|| {
        format!(
            "missing deletion count in numstat record '{}'",
            display_git_record(record)
        )
    })?;
    let path = parts.next().with_context(|| {
        format!(
            "missing path in numstat record '{}'",
            display_git_record(record)
        )
    })?;
    let binary = insertions == b"-" && deletions == b"-";

    Ok(GitDiffEntry {
        path: pathbuf_from_git_path(path)?,
        insertions: (!binary)
            .then(|| parse_numstat_count(insertions, "insertion", record))
            .transpose()?,
        deletions: (!binary)
            .then(|| parse_numstat_count(deletions, "deletion", record))
            .transpose()?,
        binary,
    })
}

fn parse_numstat_count(bytes: &[u8], field: &str, record: &[u8]) -> Result<u64> {
    str::from_utf8(bytes)
        .with_context(|| {
            format!(
                "{field} count is not UTF-8 in numstat record '{}'",
                display_git_record(record)
            )
        })?
        .parse()
        .with_context(|| {
            format!(
                "invalid {field} count in numstat record '{}'",
                display_git_record(record)
            )
        })
}

#[cfg(unix)]
fn pathbuf_from_git_path(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(unix))]
fn pathbuf_from_git_path(bytes: &[u8]) -> Result<PathBuf> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .context("git returned a non-UTF-8 path on this platform")
}

fn display_git_record(record: &[u8]) -> String {
    String::from_utf8_lossy(record).into_owned()
}

fn serialize_path_lossy<S>(path: &Path, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.to_string_lossy())
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
    let output = run_git_bytes(repo_root, args)?;

    String::from_utf8(output).context("git returned non-UTF-8 output")
}

fn run_git_bytes<const N: usize>(repo_root: &Path, args: [&str; N]) -> Result<Vec<u8>> {
    run_git_bytes_with_config(repo_root, &[], args)
}

fn run_git_bytes_with_config<const N: usize>(
    repo_root: &Path,
    config_overrides: &[(String, String)],
    args: [&str; N],
) -> Result<Vec<u8>> {
    let output = git_command_with_config(repo_root, config_overrides, args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(output.stdout)
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
    git_command_with_config(repo_root, &[], args)
}

fn git_command_with_config<const N: usize>(
    repo_root: &Path,
    config_overrides: &[(String, String)],
    args: [&str; N],
) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.fsmonitor=false")
        .args(
            config_overrides
                .iter()
                .flat_map(|(key, value)| ["-c".to_string(), format!("{key}={value}")]),
        )
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
    fn diff_summary_reports_staged_and_unstaged_counts_without_patch_contents() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "one\ntwo\n");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write("tracked.txt", "one\ntwo\nthree\n");
        repo.write("staged.txt", "alpha\nbeta\n");
        repo.git(["add", "staged.txt"]);

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.staged.files, 1);
        assert_eq!(diff.staged.insertions, 2);
        assert_eq!(diff.staged.deletions, 0);
        assert_eq!(diff.unstaged.files, 1);
        assert_eq!(diff.unstaged.insertions, 1);
        assert_eq!(diff.unstaged.deletions, 0);
        assert!(!diff.patch_contents_included);
        assert!(!diff.remote_network_used);

        let serialized = serde_json::to_string(&diff).expect("serialize diff summary");
        assert!(!serialized.contains("alpha"));
        assert!(!serialized.contains("three"));
    }

    #[test]
    fn diff_summary_reports_binary_files_without_readable_counts() {
        let repo = TestRepo::new();
        repo.write_bytes("image.bin", b"\0\x01\x02\x03");
        repo.git(["add", "image.bin"]);

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.staged.files, 1);
        assert_eq!(diff.staged.insertions, 0);
        assert_eq!(diff.staged.deletions, 0);
        assert_eq!(
            diff.staged.entries,
            vec![GitDiffEntry {
                path: PathBuf::from("image.bin"),
                insertions: None,
                deletions: None,
                binary: true,
            }]
        );
    }

    #[test]
    fn diff_summary_preserves_paths_with_tabs() {
        let repo = TestRepo::new();
        repo.write("tab\tfile.txt", "hello\n");
        repo.git(["add", "tab\tfile.txt"]);

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.staged.files, 1);
        assert_eq!(diff.staged.entries[0].path, PathBuf::from("tab\tfile.txt"));
    }

    #[test]
    fn diff_summary_does_not_execute_external_diff_or_textconv() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "hello\n");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write(
            "diff-helper.sh",
            "#!/bin/sh\necho helper-ran >> \"$PWD/diff-helper.log\"\ncat \"$1\" >/dev/null 2>/dev/null || true\n",
        );
        repo.chmod_executable("diff-helper.sh");
        repo.git(["config", "diff.external", "./diff-helper.sh"]);
        repo.git(["config", "diff.spy.textconv", "./diff-helper.sh"]);
        repo.write(".gitattributes", "*.txt diff=spy\n");
        repo.git(["add", ".gitattributes"]);
        repo.git(["commit", "-m", "Add diff attributes"]);
        repo.remove("diff-helper.log");
        repo.write("tracked.txt", "hello\nworld\n");

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.unstaged.files, 1);
        assert!(!repo.path().join("diff-helper.log").exists());
        assert!(!diff.remote_network_used);
    }

    #[test]
    fn diff_summary_does_not_execute_clean_filters() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "hello\n");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write("filter.sh", "#!/bin/sh\ntee \"$PWD/filter.log\"\n");
        repo.chmod_executable("filter.sh");
        repo.git(["config", "filter.spy.clean", "./filter.sh"]);
        repo.git(["config", "filter.spy.required", "true"]);
        repo.write(".gitattributes", "*.txt filter=spy\n");
        repo.git(["add", ".gitattributes"]);
        repo.git(["commit", "-m", "Add attributes"]);
        repo.git(["config", "filter.spy.process", "./filter.sh"]);
        repo.remove("filter.log");
        repo.write("tracked.txt", "hello\nworld\n");

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.unstaged.files, 1);
        assert!(!repo.path().join("filter.log").exists());
        assert!(!diff.remote_network_used);
    }

    #[test]
    fn diff_summary_disables_repo_fsmonitor() {
        let repo = TestRepo::new();
        repo.write("tracked.txt", "hello\n");
        repo.git(["add", "tracked.txt"]);
        repo.git(["commit", "-m", "Add tracked file"]);
        repo.write(
            "fsmonitor.sh",
            "#!/bin/sh\necho fsmonitor-ran >> \"$PWD/fsmonitor.log\"\n",
        );
        repo.chmod_executable("fsmonitor.sh");
        repo.git(["config", "core.fsmonitor", "./fsmonitor.sh"]);
        repo.remove("fsmonitor.log");
        repo.write("tracked.txt", "hello\nworld\n");

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.unstaged.files, 1);
        assert!(!repo.path().join("fsmonitor.log").exists());
        assert!(!diff.remote_network_used);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn diff_summary_handles_non_utf8_paths() {
        use std::ffi::{OsStr, OsString};
        use std::os::unix::ffi::OsStringExt;

        let repo = TestRepo::new();
        let path = OsString::from_vec(b"non-utf8-\xFF.txt".to_vec());
        fs::write(repo.path().join(&path), "hello\n").expect("write file");
        repo.git_os([OsStr::new("add"), path.as_os_str()]);

        let diff = diff_summary(repo.path()).expect("diff summary");

        assert_eq!(diff.staged.files, 1);
        assert_eq!(diff.staged.entries[0].path, PathBuf::from(path));
    }

    #[cfg(unix)]
    #[test]
    fn diff_summary_serializes_non_utf8_paths_lossily() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let diff = GitDiffSummary {
            repo_root: PathBuf::from("."),
            staged: GitDiffScopeSummary {
                files: 1,
                insertions: 1,
                deletions: 0,
                entries: vec![GitDiffEntry {
                    path: PathBuf::from(OsString::from_vec(b"non-utf8-\xFF.txt".to_vec())),
                    insertions: Some(1),
                    deletions: Some(0),
                    binary: false,
                }],
            },
            unstaged: GitDiffScopeSummary::default(),
            captured_at_unix_ms: 0,
            patch_contents_included: false,
            remote_network_used: false,
        };

        let serialized = serde_json::to_string(&diff).expect("serialize diff summary");

        assert!(serialized.contains("non-utf8-"));
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

        fn write_bytes(&self, relative: &str, contents: &[u8]) {
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
            self.git_os(args);
        }

        fn git_os<I, S>(&self, args: I)
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
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
