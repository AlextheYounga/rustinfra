use std::error::Error;

use crate::api::{CommandBit, StringCommand, USER_HOME_PATH_PREFIX};

use super::Fact;

mod find;
pub use find::{FindInFile, FindInFileInput};

const LINUX_STAT_COMMAND: &str = "stat -c 'user=%U group=%G mode=%A atime=%X mtime=%Y ctime=%Z size=%s %N'";
const BSD_STAT_COMMAND: &str = "stat -f 'user=%Su group=%Sg mode=%Sp atime=%a mtime=%m ctime=%c size=%z %N%SY'";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathInput(pub String);

impl From<&str> for PathInput {
    fn from(path: &str) -> Self {
        Self(path.to_owned())
    }
}

impl From<String> for PathInput {
    fn from(path: String) -> Self {
        Self(path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInfo {
    pub user: String,
    pub group: String,
    pub mode: u16,
    pub accessed_at: Option<i64>,
    pub modified_at: Option<i64>,
    pub changed_at: Option<i64>,
    pub size: u64,
    pub link_target: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathInfo {
    Missing,
    DifferentType,
    Present(FileInfo),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathType {
    File,
    Directory,
    Link,
}

macro_rules! path_fact {
    ($name:ident, $fact_name:literal, $path_type:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = PathInput;
            type Output = PathInfo;

            fn name(&self) -> &'static str {
                $fact_name
            }

            fn command(&self, input: &Self::Input) -> StringCommand {
                path_info_command(&input.0)
            }

            fn default(&self) -> Self::Output {
                PathInfo::Missing
            }

            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(parse_path_info(output, $path_type))
            }
        }
    };
}

path_fact!(File, "files.File", PathType::File);
path_fact!(Directory, "files.Directory", PathType::Directory);
path_fact!(Link, "files.Link", PathType::Link);

fn path_info_command(path: &str) -> StringCommand {
    let path = shell_path(path);
    StringCommand::new([format!(
        "! (test -e {path} || test -L {path} ) || ( {LINUX_STAT_COMMAND} {path} 2> /dev/null || {BSD_STAT_COMMAND} {path} || ls -ld {path} )"
    )])
}

fn shell_path(path: &str) -> String {
    if let Some(relative) = path.strip_prefix(USER_HOME_PATH_PREFIX) {
        format!("{USER_HOME_PATH_PREFIX}{}", quoted(relative))
    } else {
        quoted(path)
    }
}

fn quoted(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value.to_owned())]).raw_value()
}

fn parse_path_info(output: &[String], expected: PathType) -> PathInfo {
    let Some(line) = output.first() else {
        return PathInfo::Missing;
    };
    let Some((actual, info)) = parse_stat_line(line) else {
        return PathInfo::Missing;
    };
    if actual == expected { PathInfo::Present(info) } else { PathInfo::DifferentType }
}

fn parse_stat_line(line: &str) -> Option<(PathType, FileInfo)> {
    let line = line.strip_prefix("user=")?;
    let (user, line) = line.split_once(" group=")?;
    let (group, line) = line.split_once(" mode=")?;
    let (mode, line) = line.split_once(" atime=")?;
    let (accessed_at, line) = line.split_once(" mtime=")?;
    let (modified_at, line) = line.split_once(" ctime=")?;
    let (changed_at, line) = line.split_once(" size=")?;
    let (size, name) = line.split_once(' ')?;
    let actual = match mode.chars().next()? {
        '-' => PathType::File,
        'd' => PathType::Directory,
        'l' => PathType::Link,
        _ => return None,
    };
    let link_target = (actual == PathType::Link)
        .then(|| name.split_once(" -> ").map(|(_, target)| unquote_stat_name(target)))
        .flatten();
    Some((
        actual,
        FileInfo {
            user: user.to_owned(),
            group: group.to_owned(),
            mode: parse_mode(mode.get(1..)?)?,
            accessed_at: accessed_at.parse().ok(),
            modified_at: modified_at.parse().ok(),
            changed_at: changed_at.parse().ok(),
            size: size.parse().ok()?,
            link_target,
        },
    ))
}

fn parse_mode(mode: &str) -> Option<u16> {
    let characters = mode.chars().take(9).collect::<Vec<_>>();
    if characters.len() != 9 {
        return None;
    }
    let mut value = 0_u16;
    for (index, character) in characters.into_iter().enumerate() {
        let bit = match (index % 3, character) {
            (0, 'r') => 4,
            (1, 'w') => 2,
            (2, 'x' | 's' | 't') => 1,
            (_, '-') | (2, 'S' | 'T') => 0,
            _ => return None,
        };
        value += bit * 10_u16.pow(u32::try_from(2 - index / 3).ok()?);
        if index == 2 && matches!(character, 's' | 'S') {
            value += 4000;
        } else if index == 5 && matches!(character, 's' | 'S') {
            value += 2000;
        } else if index == 8 && matches!(character, 't' | 'T') {
            value += 1000;
        }
    }
    Some(value)
}

fn unquote_stat_name(value: &str) -> String {
    [("'", "'"), ("`", "'"), ("\"", "\"")]
        .into_iter()
        .find_map(|(start, end)| value.strip_prefix(start)?.strip_suffix(end))
        .unwrap_or(value)
        .to_owned()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FileContents;

impl Fact for FileContents {
    type Input = PathInput;
    type Output = Option<Vec<String>>;

    fn name(&self) -> &'static str {
        "files.FileContents"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        let path = quoted(&input.0);
        let missing = quoted(&format!("__pyinfra_missing_{}", input.0));
        StringCommand::new([format!("( test -e {path} && cat {path} ) || echo {missing}")])
    }

    fn default(&self) -> Self::Output {
        None
    }

    fn process(&self, output: &[String], input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let marker = format!("__pyinfra_missing_{}", input.0);
        Ok((output.first().map(String::as_str) != Some(marker.as_str())).then(|| output.to_vec()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SameFileInput {
    pub path: String,
    pub target: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SameFile;

impl Fact for SameFile {
    type Input = SameFileInput;
    type Output = bool;

    fn name(&self) -> &'static str {
        "files.SameFile"
    }

    fn command(&self, input: &Self::Input) -> StringCommand {
        StringCommand::new([format!(
            "test {} -ef {} && echo true || echo false",
            quoted(&input.path),
            quoted(&input.target)
        )])
    }

    fn default(&self) -> Self::Output {
        false
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output == ["true"])
    }
}

macro_rules! hash_fact {
    ($name:ident, $fact_name:literal, $digits:literal, $commands:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = PathInput;
            type Output = Option<String>;

            fn name(&self) -> &'static str {
                $fact_name
            }

            fn command(&self, input: &Self::Input) -> StringCommand {
                let path = quoted(&input.0);
                StringCommand::new([format!("test -e {path} && ( {} ) || true", $commands.replace("{path}", &path))])
            }

            fn default(&self) -> Self::Output {
                None
            }

            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(output.first().and_then(|line| parse_hash(line, $digits)))
            }
        }
    };
}

hash_fact!(
    Sha1File,
    "files.Sha1File",
    40,
    "sha1sum < {path} 2> /dev/null || shasum < {path} 2> /dev/null || sha1 < {path} 2> /dev/null"
);
hash_fact!(
    Sha256File,
    "files.Sha256File",
    64,
    "sha256sum < {path} 2> /dev/null || shasum -a 256 < {path} 2> /dev/null || sha256 < {path} 2> /dev/null"
);
hash_fact!(
    Sha384File,
    "files.Sha384File",
    96,
    "sha384sum < {path} 2> /dev/null || shasum -a 384 < {path} 2> /dev/null || sha384 < {path} 2> /dev/null"
);
hash_fact!(Md5File, "files.Md5File", 32, "md5sum < {path} 2> /dev/null || md5 < {path} 2> /dev/null");

fn parse_hash(line: &str, digits: usize) -> Option<String> {
    let hash = line.split_whitespace().next()?;
    (hash.len() == digits && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| hash.to_owned())
}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::{Fact, File, FileContents, PathInfo, PathInput, SameFile, SameFileInput, Sha256File};

    #[test]
    fn file_matches_reference_command_and_parses_stat_with_spaced_group() {
        let input = PathInput::from("fil () &&-@_e.txt");
        let output = [
            "user=pyinfra group=domain users mode=-rwxrwx--- atime=1594804767 mtime=1594804767 ctime=0 size=8 'fil () &&-@_e.txt'"
                .to_owned(),
        ];

        assert!(File.command(&input).raw_value().contains("test -e 'fil () &&-@_e.txt'"));
        let PathInfo::Present(info) = File.process(&output, &input).unwrap() else {
            panic!("expected file information");
        };
        assert_eq!(info.group, "domain users");
        assert_eq!(info.mode, 770);
        assert_eq!(info.size, 8);
    }

    #[test]
    fn tilde_expansion_file_contents_and_same_file_preserve_reference_behavior() {
        assert!(File.command(&PathInput::from("~/My Documents/file")).raw_value().contains("~/'My Documents/file'"));
        assert_eq!(
            FileContents.process(&["__pyinfra_missing_/missing".to_owned()], &PathInput::from("/missing")).unwrap(),
            None
        );
        let same = SameFileInput { path: "one path".to_owned(), target: "target".to_owned() };
        assert_eq!(SameFile.command(&same).raw_value(), "test 'one path' -ef target && echo true || echo false");
    }

    #[test]
    fn hashes_read_via_stdin_and_accept_gnu_and_bsd_output() {
        let input = PathInput::from("file needs quotes");
        let hash = "a".repeat(64);

        assert!(Sha256File.command(&input).raw_value().contains("sha256sum < 'file needs quotes'"));
        assert_eq!(Sha256File.process(&[format!("{hash}  -")], &input).unwrap(), Some(hash.clone()));
        assert_eq!(Sha256File.process(slice::from_ref(&hash), &input).unwrap(), Some(hash));
        assert_eq!(Sha256File.process(&["not-a-hash".to_owned()], &input).unwrap(), None);
    }
}
