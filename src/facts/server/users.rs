use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;
use crate::facts::Fact;

const USERS_COMMAND: &str = "for i in `cat /etc/passwd | cut -d: -f1`; do\n    ENTRY=`USER=\"$i\" awk -F: '$1 == ENVIRON[\"USER\"] {print}' /etc/passwd`;\n    LASTLOG=`(((lastlog -u $i || lastlogin $i) 2> /dev/null) | USER=\"$i\" awk '$1 == ENVIRON[\"USER\"] {print}' | tr -s ' ')`;\n    PASSWORD=`(USER=\"$i\" awk -F: '$1 == ENVIRON[\"USER\"] {print}' /etc/shadow || USER=\"$i\" awk -F: '$1 == ENVIRON[\"USER\"] {print}' /etc/master.passwd) 2> /dev/null | cut -d: -f2`;\n    echo \"$ENTRY|`id -gn $i`|`id -Gn $i`|$LASTLOG|$PASSWORD\";\ndone";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserInfo {
    pub home: Option<String>,
    pub comment: Option<String>,
    pub shell: Option<String>,
    pub group: String,
    pub groups: Vec<String>,
    pub uid: u64,
    pub gid: u64,
    pub lastlog: Option<String>,
    pub password: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Users;

impl Fact for Users {
    type Input = ();
    type Output = IndexMap<String, UserInfo>;

    fn name(&self) -> &'static str {
        "server.Users"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new([USERS_COMMAND])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut users = IndexMap::new();
        for line in output {
            let fields = line.rsplitn(5, '|').collect::<Vec<_>>();
            let [password, lastlog, user_groups, group, entry] = fields.as_slice() else {
                continue;
            };
            if entry.is_empty() {
                continue;
            }
            let entries = entry.split(':').collect::<Vec<_>>();
            let [name, _, uid, gid, comment, home, shell, ..] = entries.as_slice() else {
                continue;
            };
            let (Ok(uid), Ok(gid)) = (uid.parse(), gid.parse()) else {
                continue;
            };
            let groups =
                user_groups.split_whitespace().filter(|user_group| user_group != group).map(str::to_owned).collect();
            users.insert(
                (*name).to_owned(),
                UserInfo {
                    home: nonempty(home),
                    comment: nonempty(comment),
                    shell: nonempty(shell),
                    group: (*group).to_owned(),
                    groups,
                    uid,
                    gid,
                    lastlog: login_date(lastlog),
                    password: (*password).to_owned(),
                },
            );
        }
        Ok(users)
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn login_date(lastlog: &str) -> Option<String> {
    const MONTHS: &[&str] = &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let words = lastlog.split_whitespace().collect::<Vec<_>>();
    let month = words.iter().position(|word| MONTHS.contains(word))?;
    let end = (month + 6).min(words.len());
    Some(words[month..end].join(" "))
}

#[cfg(test)]
mod tests {
    use super::{Fact, Users};

    #[test]
    fn parses_reference_user_shape_and_secondary_groups() {
        let output = [
            "root:x:0:0:root:/root:/bin/ksh|wheel|wheel kmem sys|root pts/0 Sat Jun 5 12:03:23 -0600 2021|$rootpw$"
                .to_owned(),
            "noshell:x:1002:1002:noshell comment with spaces:/home/noshell:|noshell||noshell **Never logged in**|"
                .to_owned(),
        ];
        let users = Users.process(&output, &()).unwrap();

        assert_eq!(users["root"].groups, ["kmem", "sys"]);
        assert_eq!(users["root"].lastlog.as_deref(), Some("Jun 5 12:03:23 -0600 2021"));
        assert_eq!(users["noshell"].shell, None);
        assert_eq!(users["noshell"].password, "");
    }
}
