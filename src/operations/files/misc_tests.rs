use super::*;
use crate::facts::files::FileInfo;

fn text(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(value) => Some(value.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn move_rename_copy_and_flags_preserve_reference_commands() {
    let mut move_input = MoveInput::new("tmp/source file", "tmp/dest dir");
    move_input.overwrite = true;
    assert_eq!(
        text(
            &move_commands(
                &move_input,
                "tmp/dest dir/source file",
                &PathInfo::Missing,
                &PathInfo::Present(fake_info())
            )
            .unwrap()
        ),
        ["rm -rf 'tmp/dest dir/source file'", "mv 'tmp/source file' 'tmp/dest dir'"]
    );
    let rename = RenameInput::new("tmp/a", "tmp/b");
    assert!(matches!(
        rename_commands(&rename, &PathInfo::Missing, &PathInfo::Missing),
        Err(FilesOperationError::MissingSource(_))
    ));
    let copy = CopyInput::new("/tmp/source", "/tmp/dest");
    let state = CopyState {
        target_path: "/tmp/dest/source".to_owned(),
        directory: PathInfo::Present(fake_info()),
        target: PathInfo::Present(fake_info()),
        equal: true,
    };
    assert!(copy_commands(&copy, &state).unwrap().is_empty());
    let mut flags = FlagsInput::new("my file");
    flags.flags = vec!["hidden".to_owned(), "dump".to_owned()];
    assert_eq!(text(&flags_commands(&flags, &["dump".to_owned()])), ["chflags hidden 'my file'"]);
}

#[test]
fn replace_quotes_inputs() {
    let mut replace = ReplaceInput::new("my file", "old/value", "new&value");
    replace.flags = vec!["g".to_owned()];
    assert_eq!(
        text(&[replace_command(&replace)]),
        [
            "sed -i.rustinfra-replace-backup 's/old\\/value/new\\&value/g' 'my file' && rm -f 'my file.rustinfra-replace-backup'"
        ]
    );
}

fn fake_info() -> FileInfo {
    FileInfo {
        user: "root".to_owned(),
        group: "root".to_owned(),
        mode: 600,
        accessed_at: None,
        modified_at: None,
        changed_at: None,
        size: 0,
        link_target: None,
    }
}
