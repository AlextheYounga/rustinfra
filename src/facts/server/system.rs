use std::error::Error;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};
use crate::facts::Fact;

use super::{HomeInput, LinuxDistribution};

pub(crate) const ETC_HOSTS_PATH: &str = "/etc/hosts";

fn quote(value: &str) -> String {
    StringCommand::new([CommandBit::quoted(value)]).raw_value()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelInput {
    pub kernel: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Uptime;
impl Fact for Uptime {
    type Input = KernelInput;
    type Output = u64;
    fn name(&self) -> &'static str {
        "server.Uptime"
    }
    fn command(&self, input: &KernelInput) -> StringCommand {
        if matches!(input.kernel.as_str(), "Darwin" | "FreeBSD") {
            StringCommand::new(["date +%s; sysctl -n kern.boottime | awk '{print $4}' | tr -d ','"])
        } else {
            StringCommand::new(["cut -d. -f1 /proc/uptime"])
        }
    }
    fn default(&self) -> u64 {
        0
    }
    fn process(&self, output: &[String], input: &KernelInput) -> Result<u64, Box<dyn Error + Send + Sync>> {
        let first = output.first().map(String::as_str).unwrap_or_default().parse::<u64>()?;
        if matches!(input.kernel.as_str(), "Darwin" | "FreeBSD") {
            Ok(first.saturating_sub(output.get(1).map(String::as_str).unwrap_or_default().parse::<u64>()?))
        } else {
            Ok(first)
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthorizedKeysInput {
    pub user: HomeInput,
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AuthorizedKeys;
impl Fact for AuthorizedKeys {
    type Input = AuthorizedKeysInput;
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "server.AuthorizedKeys"
    }
    fn command(&self, input: &AuthorizedKeysInput) -> StringCommand {
        input.path.as_deref().map_or_else(
            || StringCommand::new([format!("cat ~{}/.ssh/authorized_keys 2>/dev/null || true", input.user.0)]),
            |path| StringCommand::new([format!("cat {} 2>/dev/null || true", quote(path))]),
        )
    }
    fn default(&self) -> Vec<String> {
        Vec::new()
    }
    fn process(&self, output: &[String], _: &AuthorizedKeysInput) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EtcHosts;
impl Fact for EtcHosts {
    type Input = Option<String>;
    type Output = IndexMap<String, Vec<String>>;
    fn name(&self) -> &'static str {
        "server.EtcHosts"
    }
    fn command(&self, input: &Option<String>) -> StringCommand {
        StringCommand::new([format!("cat {} 2>/dev/null || true", quote(input.as_deref().unwrap_or(ETC_HOSTS_PATH)))])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], _: &Option<String>) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut result = IndexMap::<String, Vec<String>>::new();
        for line in output {
            let values = line.split('#').next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
            if let [ip, names @ ..] = values.as_slice() {
                if !names.is_empty() {
                    let existing = result.entry((*ip).to_owned()).or_default();
                    for name in names {
                        if !existing.iter().any(|value| value == name) {
                            existing.push((*name).to_owned());
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LsbRelease;
impl Fact for LsbRelease {
    type Input = ();
    type Output = IndexMap<String, String>;
    fn name(&self) -> &'static str {
        "server.LsbRelease"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["lsb_release -ca"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("lsb_release")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| {
                (key.split_whitespace().last().unwrap_or_default().to_ascii_lowercase(), value.trim().to_owned())
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxName;
impl Fact for LinuxName {
    type Input = ();
    type Output = Option<String>;
    fn name(&self) -> &'static str {
        "server.LinuxName"
    }
    fn command(&self, input: &()) -> StringCommand {
        LinuxDistribution.command(input)
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn process(&self, output: &[String], input: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(LinuxDistribution.process(output, input)?.name)
    }
}

fn gui_names(output: &[String]) -> Vec<String> {
    output
        .iter()
        .filter_map(|line| match line.as_str() {
            "/usr/bin/gnome-session" => Some("GNOME"),
            "/usr/bin/mate-session" => Some("MATE"),
            "/usr/bin/lxsession" => Some("LXDE"),
            "/usr/bin/plasma_session" => Some("KDE Plasma"),
            "/usr/bin/xfce4-session" => Some("XFCE 4"),
            _ => None,
        })
        .map(str::to_owned)
        .collect()
}

macro_rules! gui_fact {
    ($name:ident, $fact_name:literal, $output:ty, $process:expr, $default:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = $output;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _: &()) -> StringCommand {
                StringCommand::new(["ls /usr/bin/*session || true"])
            }
            fn default(&self) -> Self::Output {
                $default
            }
            fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok($process(output))
            }
        }
    };
}
gui_fact!(LinuxGui, "server.LinuxGui", Vec<String>, gui_names, Vec::new());
gui_fact!(HasGui, "server.HasGui", bool, |output: &[String]| !gui_names(output).is_empty(), false);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelinuxInfo {
    pub mode: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Selinux;
impl Fact for Selinux {
    type Input = ();
    type Output = SelinuxInfo;
    fn name(&self) -> &'static str {
        "server.Selinux"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["sestatus"])
    }
    fn default(&self) -> SelinuxInfo {
        SelinuxInfo::default()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("sestatus")
    }
    fn process(&self, output: &[String], _: &()) -> Result<SelinuxInfo, Box<dyn Error + Send + Sync>> {
        let mode = output
            .first()
            .and_then(|line| line.strip_prefix("SELinux status:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        Ok(SelinuxInfo { mode })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityLimit {
    pub domain: String,
    pub limit_type: String,
    pub item: String,
    pub value: String,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct SecurityLimits;
impl Fact for SecurityLimits {
    type Input = ();
    type Output = Vec<SecurityLimit>;
    fn name(&self) -> &'static str {
        "server.SecurityLimits"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["! test -e /etc/security/limits.conf || cat /etc/security/limits.conf"])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let values = line.split_whitespace().collect::<Vec<_>>();
                match values.as_slice() {
                    [domain, limit_type, item, value] => Some(SecurityLimit {
                        domain: (*domain).to_owned(),
                        limit_type: (*limit_type).to_owned(),
                        item: (*item).to_owned(),
                        value: (*value).to_owned(),
                    }),
                    _ => None,
                }
            })
            .collect())
    }
}

const REBOOT_COMMAND: &str = r#"
# Get OS type
OS_TYPE=$(uname -s)
if [ "$OS_TYPE" = "Linux" ]; then
    # Check if it's Alpine Linux
    if [ -f /etc/alpine-release ]; then
        RUNNING_KERNEL=$(uname -r)
        INSTALLED_KERNEL=$(ls -1 /lib/modules | sort -V | tail -n1)
        if [ "$RUNNING_KERNEL" != "$INSTALLED_KERNEL" ]; then
            echo "reboot_required"
            exit 0
        fi
    else
        # Check standard Linux reboot required files
        if [ -f /var/run/reboot-required ] || [ -f /var/run/reboot-required.pkgs ]; then
            echo "reboot_required"
            exit 0
        fi
    fi
elif [ "$OS_TYPE" = "FreeBSD" ]; then
    RUNNING_VERSION=$(freebsd-version -r)
    INSTALLED_VERSION=$(freebsd-version -k)
    if [ "$RUNNING_VERSION" != "$INSTALLED_VERSION" ]; then
        echo "reboot_required"
        exit 0
    fi
fi
echo "no_reboot_required"
"#;

#[derive(Clone, Copy, Debug, Default)]
pub struct RebootRequired;
impl Fact for RebootRequired {
    type Input = ();
    type Output = bool;
    fn name(&self) -> &'static str {
        "server.RebootRequired"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([REBOOT_COMMAND])
    }
    fn default(&self) -> bool {
        false
    }
    fn process(&self, output: &[String], _: &()) -> Result<bool, Box<dyn Error + Send + Sync>> {
        Ok(output.first().is_some_and(|line| line.trim() == "reboot_required"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_commands_and_parsing_match_linux_and_bsd() {
        let linux = KernelInput { kernel: "Linux".to_owned() };
        assert_eq!(Uptime.command(&linux).raw_value(), "cut -d. -f1 /proc/uptime");
        assert_eq!(Uptime.process(&["86400".to_owned()], &linux).unwrap(), 86_400);
        let darwin = KernelInput { kernel: "Darwin".to_owned() };
        assert_eq!(Uptime.process(&["1746748800".to_owned(), "1746662400".to_owned()], &darwin).unwrap(), 86_400);
    }

    #[test]
    fn authorized_keys_and_hosts_quote_paths_and_parse_reference_shapes() {
        let input = AuthorizedKeysInput { user: HomeInput::try_from("alice").unwrap(), path: None };
        assert_eq!(AuthorizedKeys.command(&input).raw_value(), "cat ~alice/.ssh/authorized_keys 2>/dev/null || true");
        assert_eq!(
            AuthorizedKeys.process(&["# comment".to_owned(), " ssh-ed25519 key user ".to_owned()], &input).unwrap(),
            ["ssh-ed25519 key user"]
        );
        let path = Some("/tmp/host file".to_owned());
        assert_eq!(EtcHosts.command(&path).raw_value(), "cat '/tmp/host file' 2>/dev/null || true");
        let hosts = EtcHosts
            .process(&["127.0.0.1 localhost localhost".to_owned(), "127.0.0.1 local # note".to_owned()], &None)
            .unwrap();
        assert_eq!(hosts["127.0.0.1"], ["localhost", "local"]);
    }

    #[test]
    fn release_gui_selinux_limits_and_reboot_match_fixtures() {
        let release = LsbRelease
            .process(&["Distributor ID: Ubuntu".to_owned(), "Description: Ubuntu LTS".to_owned()], &())
            .unwrap();
        assert_eq!(release["id"], "Ubuntu");
        assert_eq!(
            LinuxName.process(&["/etc/os-release".to_owned(), "NAME=\"Ubuntu\"".to_owned()], &()).unwrap().as_deref(),
            Some("Ubuntu")
        );
        assert_eq!(LinuxGui.process(&["/usr/bin/gnome-session".to_owned()], &()).unwrap(), ["GNOME"]);
        assert!(HasGui.process(&["/usr/bin/gnome-session".to_owned()], &()).unwrap());
        assert_eq!(
            Selinux.process(&["SELinux status: enabled".to_owned()], &()).unwrap().mode.as_deref(),
            Some("enabled")
        );
        let limits = SecurityLimits.process(&["# comment".to_owned(), "* soft nofile 104857".to_owned()], &()).unwrap();
        assert_eq!(limits[0].item, "nofile");
        assert!(RebootRequired.process(&["reboot_required".to_owned()], &()).unwrap());
    }
}
