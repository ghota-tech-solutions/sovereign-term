use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, Serializer};

const DEFAULT_MAX_DEPTH: usize = 4;
const DEFAULT_MAX_ENTRIES: usize = 2_000;
pub const DEFAULT_READ_PREVIEW_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTreeSnapshot {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub root: PathBuf,
    pub policy: FileSnapshotPolicy,
    pub entries: Vec<FileTreeEntry>,
    pub truncated: bool,
    pub captured_at_unix_ms: u128,
    pub file_contents_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSnapshotPolicy {
    pub max_depth: usize,
    pub max_entries: usize,
    pub include_hidden: bool,
    pub follow_symlinks: bool,
    pub ignored_names: Vec<String>,
}

impl Default for FileSnapshotPolicy {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_entries: DEFAULT_MAX_ENTRIES,
            include_hidden: false,
            follow_symlinks: false,
            ignored_names: default_ignored_names(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTreeEntry {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    pub kind: FileEntryKind,
    pub byte_len: Option<u64>,
    pub modified_unix_ms: Option<u128>,
    pub readonly: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FileEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileReadPolicy {
    pub max_bytes: usize,
    pub include_hidden: bool,
    pub ignored_names: Vec<String>,
}

impl Default for FileReadPolicy {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_READ_PREVIEW_MAX_BYTES,
            include_hidden: false,
            ignored_names: default_ignored_names(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileReadPreview {
    #[serde(serialize_with = "serialize_path_lossy")]
    pub root: PathBuf,
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    pub byte_len: u64,
    pub modified_unix_ms: Option<u128>,
    pub readonly: bool,
    pub max_bytes: usize,
    pub bytes_read: usize,
    pub truncated: bool,
    pub content: String,
    pub content_encoding: FileContentEncoding,
    pub file_contents_read: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FileContentEncoding {
    Utf8,
    Utf8Lossy,
}

pub fn snapshot_tree(
    root: impl AsRef<Path>,
    policy: FileSnapshotPolicy,
) -> Result<FileTreeSnapshot> {
    let root = root
        .as_ref()
        .canonicalize()
        .with_context(|| format!("failed to canonicalize root {}", root.as_ref().display()))?;
    let mut entries = Vec::new();
    let mut queue = VecDeque::from([(root.clone(), 0usize)]);
    let mut visited_dirs = HashSet::new();
    let mut truncated = false;

    while let Some((path, depth)) = queue.pop_front() {
        if entries.len() >= policy.max_entries {
            truncated = true;
            break;
        }

        let relative = relative_path(&root, &path)?;
        if should_skip(&relative, &policy) {
            continue;
        }
        if !parent_is_inside_root(&root, &path) {
            truncated = true;
            continue;
        }

        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        let kind = entry_kind(&metadata);
        let canonical_dir = canonical_dir_for_descent(&root, &path, kind, &policy)?;
        let can_descend = canonical_dir.is_some();

        entries.push(FileTreeEntry {
            path: relative.clone(),
            kind,
            byte_len: (kind == FileEntryKind::File).then_some(metadata.len()),
            modified_unix_ms: metadata.modified().ok().and_then(system_time_to_unix_ms),
            readonly: metadata.permissions().readonly(),
        });

        if can_descend && depth < policy.max_depth {
            let Some(canonical_dir) = canonical_dir else {
                unreachable!("can_descend guarantees canonical_dir")
            };
            if !visited_dirs.insert(canonical_dir.clone()) {
                continue;
            }

            let Some(child_budget) =
                remaining_child_budget(policy.max_entries, entries.len(), queue.len())
            else {
                truncated = true;
                continue;
            };

            let children = fs::read_dir(&canonical_dir)
                .with_context(|| format!("failed to read directory {}", canonical_dir.display()))?;
            let mut child_paths = Vec::new();
            for (inspected_children, entry) in children.enumerate() {
                if inspected_children >= child_budget {
                    truncated = true;
                    break;
                }
                let child_path = entry
                    .with_context(|| {
                        format!("failed to list directory {}", canonical_dir.display())
                    })?
                    .path();
                if !parent_is_inside_root(&root, &child_path) {
                    truncated = true;
                    continue;
                }
                child_paths.push(child_path);
            }
            child_paths.sort();

            for child_path in child_paths {
                queue.push_back((child_path, depth + 1));
            }
        } else if can_descend && depth >= policy.max_depth {
            truncated = true;
        }
    }

    Ok(FileTreeSnapshot {
        root,
        policy,
        entries,
        truncated,
        captured_at_unix_ms: now_unix_ms(),
        file_contents_read: false,
    })
}

pub fn read_preview(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
    policy: FileReadPolicy,
) -> Result<FileReadPreview> {
    let root_input = root.as_ref();
    let lexical_root = absolute_lexical_root(root_input)?;
    let root = root_input
        .canonicalize()
        .with_context(|| format!("failed to canonicalize root {}", root_input.display()))?;
    let requested_relative = requested_relative_path(&root, &lexical_root, path.as_ref())?;
    if should_skip_path(
        &requested_relative,
        policy.include_hidden,
        &policy.ignored_names,
    ) {
        bail!(
            "{} is denied by filesystem read policy",
            requested_relative.display()
        );
    }
    let path = resolve_file_child_without_symlinks(&root, &requested_relative)?;

    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    let kind = entry_kind(&metadata);
    match kind {
        FileEntryKind::File => {}
        FileEntryKind::Directory => bail!("{} is a directory", requested_relative.display()),
        FileEntryKind::Symlink => bail!("{} is a symlink", requested_relative.display()),
        FileEntryKind::Other => bail!("{} is not a regular file", requested_relative.display()),
    }

    let mut file =
        fs::File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(policy.max_bytes as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let byte_len = metadata.len();
    let truncated = byte_len > bytes.len() as u64;
    let content_encoding = if std::str::from_utf8(&bytes).is_ok() {
        FileContentEncoding::Utf8
    } else {
        FileContentEncoding::Utf8Lossy
    };

    Ok(FileReadPreview {
        root,
        path: requested_relative,
        byte_len,
        modified_unix_ms: metadata.modified().ok().and_then(system_time_to_unix_ms),
        readonly: metadata.permissions().readonly(),
        max_bytes: policy.max_bytes,
        bytes_read: bytes.len(),
        truncated,
        content: String::from_utf8_lossy(&bytes).into_owned(),
        content_encoding,
        file_contents_read: true,
    })
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?;
    if relative.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(relative.to_path_buf())
    }
}

fn parent_is_inside_root(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }

    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .is_some_and(|parent| parent.starts_with(root))
}

fn remaining_child_budget(
    max_entries: usize,
    entries_len: usize,
    queued_len: usize,
) -> Option<usize> {
    let remaining = max_entries
        .saturating_sub(entries_len)
        .saturating_sub(queued_len);
    (remaining > 0).then_some(remaining)
}

fn should_skip(relative: &Path, policy: &FileSnapshotPolicy) -> bool {
    should_skip_path(relative, policy.include_hidden, &policy.ignored_names)
}

fn should_skip_path(relative: &Path, include_hidden: bool, ignored_names: &[String]) -> bool {
    if relative == Path::new(".") {
        return false;
    }

    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy();

        ignored_names.iter().any(|ignored| ignored == &name)
            || (!include_hidden && name.starts_with('.'))
    })
}

fn entry_kind(metadata: &fs::Metadata) -> FileEntryKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        FileEntryKind::Symlink
    } else if file_type.is_dir() {
        FileEntryKind::Directory
    } else if file_type.is_file() {
        FileEntryKind::File
    } else {
        FileEntryKind::Other
    }
}

fn canonical_dir_for_descent(
    root: &Path,
    path: &Path,
    kind: FileEntryKind,
    policy: &FileSnapshotPolicy,
) -> Result<Option<PathBuf>> {
    match kind {
        FileEntryKind::Directory => canonical_dir_inside_root(root, path, policy),
        FileEntryKind::Symlink if policy.follow_symlinks => {
            let Ok(target) = path.canonicalize() else {
                return Ok(None);
            };
            if !target.starts_with(root) {
                return Ok(None);
            }
            if should_skip(&relative_path(root, &target)?, policy) {
                return Ok(None);
            }
            let Ok(target_metadata) = fs::symlink_metadata(&target) else {
                return Ok(None);
            };
            if target_metadata.is_dir() {
                Ok(Some(target))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

fn canonical_dir_inside_root(
    root: &Path,
    path: &Path,
    policy: &FileSnapshotPolicy,
) -> Result<Option<PathBuf>> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize directory {}", path.display()))?;
    if canonical.starts_with(root)
        && !should_skip(&relative_path(root, &canonical)?, policy)
        && fs::symlink_metadata(&canonical)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    {
        Ok(Some(canonical))
    } else {
        Ok(None)
    }
}

fn system_time_to_unix_ms(time: SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn now_unix_ms() -> u128 {
    system_time_to_unix_ms(SystemTime::now()).unwrap_or_default()
}

pub fn assert_path_inside_root(root: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<PathBuf> {
    let root = root
        .as_ref()
        .canonicalize()
        .with_context(|| format!("failed to canonicalize root {}", root.as_ref().display()))?;
    let path = path
        .as_ref()
        .canonicalize()
        .with_context(|| format!("failed to canonicalize path {}", path.as_ref().display()))?;

    if !path.starts_with(&root) {
        bail!("{} is outside {}", path.display(), root.display());
    }

    Ok(path)
}

fn requested_relative_path(root: &Path, lexical_root: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        if let Ok(relative) = path.strip_prefix(root) {
            return validated_requested_relative(relative);
        }
        if let Ok(relative) = path.strip_prefix(lexical_root) {
            return validated_requested_relative(relative);
        }
        bail!("{} is outside {}", path.display(), root.display());
    }

    validated_requested_relative(path)
}

fn validated_requested_relative(path: &Path) -> Result<PathBuf> {
    reject_parent_or_current_components(path)?;
    Ok(path.to_path_buf())
}

fn absolute_lexical_root(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .context("failed to read current directory")?
            .join(path)
    };
    Ok(normalize_lexical_path(&absolute))
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn reject_parent_or_current_components(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                bail!("{} contains a parent directory component", path.display())
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("{} must be relative to the workspace root", path.display())
            }
        }
    }
    Ok(())
}

fn resolve_file_child_without_symlinks(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative == Path::new(".") {
        bail!("file read path has no file name");
    }

    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("failed to read metadata for {}", current.display()))?;
        if metadata.file_type().is_symlink() {
            bail!("{} is a symlink", relative.display());
        }
    }

    if !parent_is_inside_root(root, &current) {
        bail!("{} is outside {}", relative.display(), root.display());
    }

    Ok(current)
}

fn default_ignored_names() -> Vec<String> {
    [
        ".git",
        ".hg",
        ".svn",
        ".DS_Store",
        ".codebase-memory",
        "target",
        "node_modules",
        ".next",
        ".turbo",
        ".cache",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}

fn serialize_path_lossy<S>(path: &Path, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn snapshot_collects_metadata_without_file_contents() {
        let workspace = TestWorkspace::new();
        workspace.write("src/main.rs", "fn main() { println!(\"secret\"); }\n");
        workspace.write("README.md", "# hello\n");

        let snapshot =
            snapshot_tree(workspace.path(), FileSnapshotPolicy::default()).expect("snapshot");

        assert!(!snapshot.file_contents_read);
        assert!(snapshot.entries.iter().any(|entry| {
            entry.path == Path::new("src/main.rs")
                && entry.kind == FileEntryKind::File
                && entry.byte_len.is_some()
        }));
        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("println"));
    }

    #[test]
    fn snapshot_ignores_git_target_node_modules_and_hidden_by_default() {
        let workspace = TestWorkspace::new();
        workspace.write(".git/config", "private");
        workspace.write("target/debug/app", "binary");
        workspace.write("node_modules/pkg/index.js", "module");
        workspace.write(".env", "SECRET=1");
        workspace.write("src/lib.rs", "pub fn ok() {}\n");

        let snapshot =
            snapshot_tree(workspace.path(), FileSnapshotPolicy::default()).expect("snapshot");
        let paths = snapshot
            .entries
            .iter()
            .map(|entry| entry.path.as_path())
            .collect::<Vec<_>>();

        assert!(paths.contains(&Path::new("src/lib.rs")));
        assert!(!paths.iter().any(|path| path.starts_with(".git")));
        assert!(!paths.iter().any(|path| path.starts_with("target")));
        assert!(!paths.iter().any(|path| path.starts_with("node_modules")));
        assert!(!paths.contains(&Path::new(".env")));
    }

    #[test]
    fn snapshot_marks_truncated_when_depth_limit_is_hit() {
        let workspace = TestWorkspace::new();
        workspace.write("a/b/c/deep.txt", "deep");

        let policy = FileSnapshotPolicy {
            max_depth: 1,
            ..FileSnapshotPolicy::default()
        };
        let snapshot = snapshot_tree(workspace.path(), policy).expect("snapshot");

        assert!(snapshot.truncated);
        assert!(
            !snapshot
                .entries
                .iter()
                .any(|entry| entry.path == Path::new("a/b/c/deep.txt"))
        );
    }

    #[test]
    fn snapshot_respects_max_entries_budget() {
        let workspace = TestWorkspace::new();
        for index in 0..10 {
            workspace.write(&format!("file-{index}.txt"), "metadata only");
        }

        let policy = FileSnapshotPolicy {
            max_entries: 3,
            ..FileSnapshotPolicy::default()
        };
        let snapshot = snapshot_tree(workspace.path(), policy).expect("snapshot");

        assert!(snapshot.truncated);
        assert!(snapshot.entries.len() <= 3);
    }

    #[test]
    fn snapshot_allows_zero_entry_budget() {
        let workspace = TestWorkspace::new();
        workspace.write("file.txt", "metadata only");

        let policy = FileSnapshotPolicy {
            max_entries: 0,
            ..FileSnapshotPolicy::default()
        };
        let snapshot = snapshot_tree(workspace.path(), policy).expect("snapshot");

        assert!(snapshot.truncated);
        assert!(snapshot.entries.is_empty());
    }

    #[test]
    fn assert_path_inside_root_rejects_escape() {
        let workspace = TestWorkspace::new();
        let outside = TempDir::new().expect("outside temp");
        let outside_file = outside.path().join("outside.txt");
        fs::write(&outside_file, "outside").expect("outside file");

        let error =
            assert_path_inside_root(workspace.path(), &outside_file).expect_err("outside path");

        assert!(error.to_string().contains("outside"));
    }

    #[test]
    fn read_preview_reads_bounded_content_with_audit_metadata() {
        let workspace = TestWorkspace::new();
        workspace.write("notes/today.txt", "hello sovereign terminal\n");

        let preview = read_preview(
            workspace.path(),
            "notes/today.txt",
            FileReadPolicy {
                max_bytes: 5,
                ..FileReadPolicy::default()
            },
        )
        .expect("read preview");

        assert_eq!(preview.path, Path::new("notes/today.txt"));
        assert_eq!(preview.content, "hello");
        assert_eq!(preview.bytes_read, 5);
        assert!(preview.truncated);
        assert!(preview.file_contents_read);
        assert_eq!(preview.content_encoding, FileContentEncoding::Utf8);
        assert!(preview.byte_len > preview.bytes_read as u64);

        let serialized = serde_json::to_string(&preview).expect("serialize preview");
        assert!(serialized.contains("hello"));
        assert!(!serialized.contains("sovereign terminal"));
    }

    #[test]
    fn read_preview_rejects_paths_outside_root() {
        let workspace = TestWorkspace::new();
        let outside = TempDir::new().expect("outside temp");
        let outside_file = outside.path().join("outside.txt");
        fs::write(&outside_file, "outside").expect("outside file");

        let error = read_preview(workspace.path(), &outside_file, FileReadPolicy::default())
            .expect_err("outside path");

        assert!(error.to_string().contains("outside"));
    }

    #[test]
    fn read_preview_accepts_absolute_paths_inside_root() {
        let workspace = TestWorkspace::new();
        workspace.write("src/lib.rs", "pub fn ok() {}\n");
        let root = workspace.path().canonicalize().expect("canonical root");

        let preview = read_preview(&root, root.join("src/lib.rs"), FileReadPolicy::default())
            .expect("absolute preview");

        assert_eq!(preview.path, Path::new("src/lib.rs"));
        assert_eq!(preview.content, "pub fn ok() {}\n");
    }

    #[cfg(unix)]
    #[test]
    fn read_preview_accepts_absolute_paths_with_noncanonical_root_spelling() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("temp dir");
        let real_root = temp.path().join("real-root");
        let alias_root = temp.path().join("alias-root");
        fs::create_dir_all(real_root.join("src")).expect("create real root");
        fs::write(real_root.join("src/lib.rs"), "pub fn ok() {}\n").expect("write file");
        symlink(&real_root, &alias_root).expect("root symlink");

        let preview = read_preview(
            &alias_root,
            alias_root.join("src/lib.rs"),
            FileReadPolicy::default(),
        )
        .expect("absolute preview through alias root");

        assert_eq!(
            preview.root,
            real_root.canonicalize().expect("canonical root")
        );
        assert_eq!(preview.path, Path::new("src/lib.rs"));
        assert_eq!(preview.content, "pub fn ok() {}\n");
    }

    #[test]
    fn read_preview_rejects_directories_and_hidden_paths_by_default() {
        let workspace = TestWorkspace::new();
        workspace.write("src/lib.rs", "pub fn ok() {}\n");
        workspace.write(".env", "SECRET=1\n");

        let directory_error = read_preview(workspace.path(), "src", FileReadPolicy::default())
            .expect_err("directory read");
        assert!(directory_error.to_string().contains("directory"));

        let hidden_error = read_preview(workspace.path(), ".env", FileReadPolicy::default())
            .expect_err("hidden read");
        assert!(hidden_error.to_string().contains("denied"));
    }

    #[test]
    fn read_preview_reports_lossy_encoding_for_binary_content() {
        let workspace = TestWorkspace::new();
        workspace.write_bytes("blob.bin", b"hi\xFF");

        let preview = read_preview(workspace.path(), "blob.bin", FileReadPolicy::default())
            .expect("binary preview");

        assert_eq!(preview.bytes_read, 3);
        assert_eq!(preview.content_encoding, FileContentEncoding::Utf8Lossy);
        assert!(preview.file_contents_read);
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_does_not_follow_symlinks_outside_root() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        let outside = TempDir::new().expect("outside temp");
        fs::write(outside.path().join("leak.txt"), "secret").expect("outside file");
        symlink(outside.path(), workspace.path().join("external")).expect("symlink");

        let policy = FileSnapshotPolicy {
            follow_symlinks: true,
            ..FileSnapshotPolicy::default()
        };
        let snapshot = snapshot_tree(workspace.path(), policy).expect("snapshot");

        assert!(snapshot.entries.iter().any(|entry| {
            entry.path == Path::new("external") && entry.kind == FileEntryKind::Symlink
        }));
        assert!(
            !snapshot
                .entries
                .iter()
                .any(|entry| entry.path == Path::new("external/leak.txt"))
        );

        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!serialized.contains("leak.txt"));
        assert!(!serialized.contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_does_not_follow_symlink_aliases_into_ignored_or_hidden_dirs() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        workspace.write(".git/config", "GIT_CONTENT_SHOULD_NOT_APPEAR");
        workspace.write(".secret/leak.txt", "HIDDEN_CONTENT_SHOULD_NOT_APPEAR");
        symlink(
            workspace.path().join(".git"),
            workspace.path().join("gitlink"),
        )
        .expect("git symlink");
        symlink(
            workspace.path().join(".secret"),
            workspace.path().join("visible-secret"),
        )
        .expect("hidden symlink");

        let policy = FileSnapshotPolicy {
            follow_symlinks: true,
            ..FileSnapshotPolicy::default()
        };
        let snapshot = snapshot_tree(workspace.path(), policy).expect("snapshot");

        assert!(snapshot.entries.iter().any(|entry| {
            entry.path == Path::new("gitlink") && entry.kind == FileEntryKind::Symlink
        }));
        assert!(snapshot.entries.iter().any(|entry| {
            entry.path == Path::new("visible-secret") && entry.kind == FileEntryKind::Symlink
        }));
        assert!(!snapshot.entries.iter().any(|entry| {
            entry.path.starts_with(".git")
                || is_descendant_of(&entry.path, Path::new("gitlink"))
                || entry.path.starts_with(".secret")
                || is_descendant_of(&entry.path, Path::new("visible-secret"))
        }));

        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!serialized.contains("GIT_CONTENT_SHOULD_NOT_APPEAR"));
        assert!(!serialized.contains("HIDDEN_CONTENT_SHOULD_NOT_APPEAR"));
        assert!(!serialized.contains("leak.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn read_preview_rejects_symlink_content_reads() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        let outside = TempDir::new().expect("outside temp");
        fs::write(outside.path().join("leak.txt"), "secret").expect("outside file");
        symlink(
            outside.path().join("leak.txt"),
            workspace.path().join("link.txt"),
        )
        .expect("symlink");

        let error = read_preview(workspace.path(), "link.txt", FileReadPolicy::default())
            .expect_err("symlink read");

        assert!(error.to_string().contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn read_preview_rejects_symlink_parent_components() {
        use std::os::unix::fs::symlink;

        let workspace = TestWorkspace::new();
        workspace.write("src/file.txt", "visible through alias");
        symlink(
            workspace.path().join("src"),
            workspace.path().join(".alias"),
        )
        .expect("hidden symlink");
        symlink(
            workspace.path().join("src"),
            workspace.path().join("visible-alias"),
        )
        .expect("visible symlink");

        let hidden_error = read_preview(
            workspace.path(),
            ".alias/file.txt",
            FileReadPolicy::default(),
        )
        .expect_err("hidden symlink parent");
        assert!(hidden_error.to_string().contains("denied"));

        let visible_error = read_preview(
            workspace.path(),
            "visible-alias/file.txt",
            FileReadPolicy::default(),
        )
        .expect_err("visible symlink parent");
        assert!(visible_error.to_string().contains("symlink"));

        let root = workspace.path().canonicalize().expect("canonical root");
        let absolute_hidden_error = read_preview(
            &root,
            root.join(".alias/file.txt"),
            FileReadPolicy::default(),
        )
        .expect_err("absolute hidden symlink parent");
        assert!(absolute_hidden_error.to_string().contains("denied"));

        let absolute_visible_error = read_preview(
            &root,
            root.join("visible-alias/file.txt"),
            FileReadPolicy::default(),
        )
        .expect_err("absolute visible symlink parent");
        assert!(absolute_visible_error.to_string().contains("symlink"));
    }

    struct TestWorkspace {
        dir: TempDir,
    }

    fn is_descendant_of(path: &Path, parent: &Path) -> bool {
        path.starts_with(parent) && path != parent
    }

    impl TestWorkspace {
        fn new() -> Self {
            Self {
                dir: TempDir::new().expect("temp dir"),
            }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, contents).expect("write file");
        }

        fn write_bytes(&self, relative: &str, contents: &[u8]) {
            let path = self.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, contents).expect("write file");
        }
    }
}
