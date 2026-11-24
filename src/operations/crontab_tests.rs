use super::{CronField, CrontabInput, crontab_commands, crontab_commands_with_backup};
use crate::api::Command;
use crate::facts::crontab::{CrontabFile, CrontabItem};

fn descriptions(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(command) => Some(command.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn adds_named_user_cron_with_list_schedule_and_safe_quoting() {
    let mut input = CrontabInput::new("/bin/run 'all'");
    input.user = Some("app user".to_owned());
    input.cron_name = Some("nightly".to_owned());
    input.minute = Some(CronField::List(vec!["0".to_owned(), "30".to_owned()]));
    let commands = descriptions(&crontab_commands(&input, &CrontabFile::default(), "/tmp/cron file"));

    assert_eq!(
        commands,
        [
            "echo '# pyinfra-name=nightly' >> '/tmp/cron file'",
            "echo '0,30 * * * * /bin/run '\"'\"'all'\"'\"'' >> '/tmp/cron file'",
            "crontab -u 'app user' '/tmp/cron file'"
        ]
    );
}

#[test]
fn unchanged_special_time_is_noop_and_changed_command_is_replaced() {
    let state = CrontabFile {
        items: vec![CrontabItem {
            command: Some("/bin/old".to_owned()),
            comments: vec!["# pyinfra-name=start".to_owned()],
            special_time: Some("@reboot".to_owned()),
            ..CrontabItem::default()
        }],
    };
    let mut input = CrontabInput::new("/bin/old");
    input.cron_name = Some("start".to_owned());
    input.special_time = Some("@reboot".to_owned());
    assert!(crontab_commands(&input, &state, "/tmp/cron").is_empty());

    input.command = "/bin/new".to_owned();
    let commands = descriptions(&crontab_commands(&input, &state, "/tmp/cron"));
    assert_eq!(commands.len(), 3);
    assert!(commands[1].contains("s/.*\\/bin\\/old.*/@reboot \\/bin\\/new/"));
}

#[test]
fn additions_named_jobs_users_special_times_and_noops_match_reference_fixtures() {
    let empty = CrontabFile::default();
    let input = CrontabInput::new("this_is_a_command");
    assert_eq!(
        descriptions(&crontab_commands_with_backup(&input, &empty, "_tempfile_", "a-timestamp")),
        ["echo '* * * * * this_is_a_command' >> _tempfile_", "crontab _tempfile_"]
    );

    let mut special = input.clone();
    special.special_time = Some("@reboot".to_owned());
    assert_eq!(
        descriptions(&crontab_commands_with_backup(&special, &empty, "_tempfile_", "a-timestamp")),
        ["echo '@reboot this_is_a_command' >> _tempfile_", "crontab _tempfile_"]
    );
    special.user = Some("x; reboot".to_owned());
    special.special_time = None;
    assert_eq!(
        descriptions(&crontab_commands_with_backup(&special, &empty, "_tempfile_", "a-timestamp")),
        ["echo '* * * * * this_is_a_command' >> _tempfile_", "crontab -u 'x; reboot' _tempfile_"]
    );

    let mut named = CrontabInput::new("backup /etc; rm -rf /");
    named.cron_name = Some("evil job; $(whoami)".to_owned());
    assert_eq!(
        descriptions(&crontab_commands_with_backup(&named, &empty, "_tempfile_", "a-timestamp")),
        [
            "echo '# pyinfra-name=evil job; $(whoami)' >> _tempfile_",
            "echo '* * * * * backup /etc; rm -rf /' >> _tempfile_",
            "crontab _tempfile_"
        ]
    );

    let same = CrontabFile {
        items: vec![CrontabItem {
            command: Some("this_is_a_command".to_owned()),
            minute: Some("0".to_owned()),
            hour: Some("0".to_owned()),
            month: Some("*".to_owned()),
            day_of_week: Some("*".to_owned()),
            day_of_month: Some("*".to_owned()),
            ..CrontabItem::default()
        }],
    };
    let mut scheduled = input;
    scheduled.minute = Some(CronField::from("0"));
    scheduled.hour = Some(CronField::from("0"));
    assert!(crontab_commands_with_backup(&scheduled, &same, "_tempfile_", "a-timestamp").is_empty());

    let another = CrontabFile {
        items: vec![CrontabItem { command: Some("this_is_another_command".to_owned()), ..CrontabItem::default() }],
    };
    let added = CrontabInput::new("this_is_a_command");
    assert_eq!(
        descriptions(&crontab_commands_with_backup(&added, &another, "_tempfile_", "a-timestamp")),
        [
            "crontab -l > _tempfile_",
            "echo '' >> _tempfile_",
            "echo '* * * * * this_is_a_command' >> _tempfile_",
            "crontab _tempfile_"
        ]
    );
}

#[test]
fn edits_between_schedules_special_times_names_and_wildcards_match_reference_fixtures() {
    let state = |command: &str, special: Option<&str>, comments: Vec<String>| CrontabFile {
        items: vec![CrontabItem {
            command: Some(command.to_owned()),
            special_time: special.map(str::to_owned),
            minute: Some("*".to_owned()),
            hour: Some("*".to_owned()),
            month: Some("*".to_owned()),
            day_of_week: Some("*".to_owned()),
            day_of_month: Some("*".to_owned()),
            comments,
            ..CrontabItem::default()
        }],
    };
    let mut input = CrontabInput::new("this_is_a_command");
    input.minute = Some(CronField::from(5));
    input.hour = Some(CronField::from(1));
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("this_is_a_command", Some("@reboot"), vec![]),
            "_tempfile_",
            "a-timestamp"
        )),
        [
            "crontab -l > _tempfile_",
            "sed -i.a-timestamp 's/.*this_is_a_command.*/5 1 * * * this_is_a_command/' _tempfile_ && rm -f _tempfile_.a-timestamp",
            "crontab _tempfile_"
        ]
    );

    input.minute = None;
    input.hour = None;
    input.special_time = Some("@reboot".to_owned());
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("this_is_a_command", Some("@daily"), vec![]),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp 's/.*this_is_a_command.*/@reboot this_is_a_command/' _tempfile_ && rm -f _tempfile_.a-timestamp"
    );

    input = CrontabInput::new("borg prune --glob-archives daily-* --keep-last 6");
    input.cron_name = Some("backup".to_owned());
    input.special_time = Some("@daily".to_owned());
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state(
                "borg prune --glob-archives weekly-* --keep-last 6",
                Some("@weekly"),
                vec!["# pyinfra-name=backup".to_owned()]
            ),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp 's/.*borg prune --glob-archives weekly-\\* --keep-last 6.*/@daily borg prune --glob-archives daily-* --keep-last 6/' _tempfile_ && rm -f _tempfile_.a-timestamp"
    );

    input = CrontabInput::new("echo done");
    input.cron_name = Some("anchors".to_owned());
    input.special_time = Some("@daily".to_owned());
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("echo $HOME ^x$", Some("@weekly"), vec!["# pyinfra-name=anchors".to_owned()]),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp 's/.*echo $HOME ^x$.*/@daily echo done/' _tempfile_ && rm -f _tempfile_.a-timestamp"
    );

    input = CrontabInput::new("command_to_be 'with_a_quoted_argument'");
    input.cron_name = Some("name of cron".to_owned());
    input.month = Some(CronField::from(1));
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("command_to_change", None, vec!["# pyinfra-name=name of cron".to_owned()]),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp 's/.*command_to_change.*/* * * 1 * command_to_be '\"'\"'with_a_quoted_argument'\"'\"'/' _tempfile_ && rm -f _tempfile_.a-timestamp"
    );
}

#[test]
fn removals_and_interpolated_backslashes_match_reference_fixtures() {
    let state = |command: &str| CrontabFile {
        items: vec![CrontabItem { command: Some(command.to_owned()), ..CrontabItem::default() }],
    };
    let mut input = CrontabInput::new("find /tmp -name '*.log' -delete");
    input.present = false;
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("find /tmp -name '*.log' -delete"),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp '/.*find \\/tmp -name '\"'\"'\\*\\.log'\"'\"' -delete.*/d' _tempfile_ && rm -f _tempfile_.a-timestamp"
    );

    input = CrontabInput::new("echo foo\\bar $HOME `date`");
    input.present = false;
    input.interpolate_variables = true;
    assert_eq!(
        descriptions(&crontab_commands_with_backup(
            &input,
            &state("echo foo\\bar $HOME `date`"),
            "_tempfile_",
            "a-timestamp"
        ))[1],
        "sed -i.a-timestamp \"/.*echo foo\\\\\\\\bar \\$HOME \\`date\\`.*/d\" _tempfile_ && rm -f _tempfile_.a-timestamp"
    );
}
