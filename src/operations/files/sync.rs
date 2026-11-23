use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::marker::Sync as MarkerSync;
use std::path::{Path, PathBuf};

use glob::Pattern;

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::files::{FindFiles, FindInput, FindLinks, FindLinksInput};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{Directory, DirectoryInput, File, FileInput, Link, LinkInput, Put, PutInput, REMOTE_PATH_SEPARATOR};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncInput {
    pub source: PathBuf,
    pub destination: String,
    pub user: Option<String>,
    pub group: Option<String>,
    pub mode: Option<u16>,
    pub directory_mode: Option<u16>,
    pub delete: bool,
    pub exclude: Vec<String>,
    pub exclude_directories: Vec<String>,
}

impl SyncInput {
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            user: None,
            group: None,
            mode: None,
            directory_mode: None,
            delete: false,
            exclude: Vec::new(),
            exclude_directories: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sync;

impl StatefulOperation for Sync {
    type Input = SyncInput;
    fn name(&self) -> &'static str {
        "files.sync"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let entries = collect_entries(input)?;
            let mut commands = Vec::new();
            for entry in entries.iter().filter(|entry| matches!(entry.kind, EntryKind::Directory)) {
                let mut directory = DirectoryInput::new(remote_path(&input.destination, &entry.relative));
                directory.user = input.user.clone();
                directory.group = input.group.clone();
                directory.mode = input.directory_mode.or(input.mode).or(entry.mode);
                commands.extend(Directory.commands(&directory, connector, options).await?);
            }
            for entry in entries.iter().filter(|entry| matches!(entry.kind, EntryKind::Link(_))) {
                let EntryKind::Link(target) = &entry.kind else { continue };
                let mut link = LinkInput::new(remote_path(&input.destination, &entry.relative), target.clone());
                link.user = input.user.clone();
                link.group = input.group.clone();
                link.create_remote_dir = false;
                commands.extend(Link.commands(&link, connector, options).await?);
            }
            for entry in entries.iter().filter(|entry| matches!(entry.kind, EntryKind::File)) {
                let mut put = PutInput::new(&entry.local, remote_path(&input.destination, &entry.relative));
                put.user = input.user.clone();
                put.group = input.group.clone();
                put.mode = input.mode.or(entry.mode);
                put.create_remote_dir = false;
                commands.extend(Put.commands(&put, connector, options).await?);
            }
            if input.delete {
                commands.extend(delete_commands(input, &entries, connector, options).await?);
            }
            Ok(commands)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EntryKind {
    Directory,
    File,
    Link(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    local: PathBuf,
    relative: PathBuf,
    mode: Option<u16>,
    kind: EntryKind,
}

fn collect_entries(input: &SyncInput) -> Result<Vec<Entry>, Box<dyn Error + Send + MarkerSync>> {
    if !input.source.is_dir() {
        return Err(format!("No such directory: {}", input.source.display()).into());
    }
    let file_patterns = patterns(&input.exclude)?;
    let directory_patterns = patterns(&input.exclude_directories)?;
    let mut entries = vec![Entry {
        local: input.source.clone(),
        relative: PathBuf::new(),
        mode: path_mode(&input.source),
        kind: EntryKind::Directory,
    }];
    let walk =
        WalkOptions { root: &input.source, file_patterns: &file_patterns, directory_patterns: &directory_patterns };
    visit(&walk, &input.source, &mut entries)?;
    Ok(entries)
}

struct WalkOptions<'a> {
    root: &'a Path,
    file_patterns: &'a [Pattern],
    directory_patterns: &'a [Pattern],
}

fn visit(
    walk: &WalkOptions<'_>,
    directory: &Path,
    entries: &mut Vec<Entry>,
) -> Result<(), Box<dyn Error + Send + MarkerSync>> {
    let mut children = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let path = child.path();
        let relative = path.strip_prefix(walk.root)?.to_path_buf();
        let relative_text = relative.to_string_lossy().replace('\\', REMOTE_PATH_SEPARATOR);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            entries.push(Entry {
                local: path.clone(),
                relative,
                mode: None,
                kind: EntryKind::Link(fs::read_link(path)?.to_string_lossy().into_owned()),
            });
        } else if metadata.is_dir() {
            if walk.directory_patterns.iter().any(|pattern| pattern.matches(&relative_text)) {
                continue;
            }
            entries.push(Entry { local: path.clone(), relative, mode: path_mode(&path), kind: EntryKind::Directory });
            visit(walk, &path, entries)?;
        } else if metadata.is_file() {
            let name = child.file_name().to_string_lossy().into_owned();
            if !walk.file_patterns.iter().any(|pattern| pattern.matches(&name) || pattern.matches(&relative_text)) {
                entries.push(Entry { local: path.clone(), relative, mode: path_mode(&path), kind: EntryKind::File });
            }
        }
    }
    Ok(())
}

async fn delete_commands(
    input: &SyncInput,
    entries: &[Entry],
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + MarkerSync>> {
    let facts = FactExecutor::new(connector, options);
    let remote_files = facts.get(&FindFiles, &FindInput::new(input.destination.clone())).await?;
    let remote_links = facts.get(&FindLinks, &FindLinksInput::new(input.destination.clone())).await?;
    let wanted_files: BTreeSet<_> = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
        .map(|entry| remote_path(&input.destination, &entry.relative))
        .collect();
    let wanted_links: BTreeSet<_> = entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::Link(_)))
        .map(|entry| remote_path(&input.destination, &entry.relative))
        .collect();
    let excludes = patterns(&input.exclude)?;
    let mut commands = Vec::new();
    for path in
        remote_files.into_iter().filter(|path| !wanted_files.contains(path) && !excluded_remote(path, &excludes))
    {
        let mut file = FileInput::new(path);
        file.present = false;
        commands.extend(File.commands(&file, connector, options).await?);
    }
    for path in
        remote_links.into_iter().filter(|path| !wanted_links.contains(path) && !excluded_remote(path, &excludes))
    {
        let mut link = LinkInput::new(path, "");
        link.present = false;
        commands.extend(Link.commands(&link, connector, options).await?);
    }
    Ok(commands)
}

fn patterns(values: &[String]) -> Result<Vec<Pattern>, glob::PatternError> {
    values.iter().map(|value| Pattern::new(value)).collect()
}
fn excluded_remote(path: &str, patterns: &[Pattern]) -> bool {
    patterns
        .iter()
        .any(|pattern| pattern.matches(path) || path.rsplit('/').next().is_some_and(|name| pattern.matches(name)))
}
fn remote_path(destination: &str, relative: &Path) -> String {
    let relative = relative.to_string_lossy().replace('\\', REMOTE_PATH_SEPARATOR);
    if relative.is_empty() {
        destination.to_owned()
    } else {
        format!("{}/{relative}", destination.trim_end_matches('/'))
    }
}

#[cfg(unix)]
fn path_mode(path: &Path) -> Option<u16> {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path).ok().and_then(|metadata| u16::try_from(metadata.permissions().mode() & 0o7777).ok())
}
#[cfg(not(unix))]
fn path_mode(_: &Path) -> Option<u16> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[test]
    fn local_tree_preserves_files_directories_symlinks_and_excludes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("kept")).unwrap();
        fs::write(root.path().join("kept/file.txt"), "x").unwrap();
        fs::write(root.path().join("ignored.pyc"), "x").unwrap();
        #[cfg(unix)]
        symlink("kept/file.txt", root.path().join("link")).unwrap();
        let mut input = SyncInput::new(root.path(), "/remote path");
        input.exclude.push("*.pyc".to_owned());
        let entries = collect_entries(&input).unwrap();
        assert!(entries.iter().any(|entry| entry.relative == Path::new("kept/file.txt")));
        assert!(!entries.iter().any(|entry| entry.relative == Path::new("ignored.pyc")));
        #[cfg(unix)]
        assert!(
            entries.iter().any(|entry| matches!(&entry.kind, EntryKind::Link(target) if target == "kept/file.txt"))
        );
        assert_eq!(remote_path(&input.destination, Path::new("kept/file.txt")), "/remote path/kept/file.txt");
    }
}
