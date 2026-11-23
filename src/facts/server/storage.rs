use std::error::Error;

use chrono::{DateTime, FixedOffset, Local};
use indexmap::IndexMap;
use serde_json::Value;

use crate::api::StringCommand;
use crate::facts::Fact;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ServerKernel {
    FreeBsd,
    OpenBsd,
    Darwin,
    #[default]
    Linux,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Date;
impl Fact for Date {
    type Input = ();
    type Output = DateTime<FixedOffset>;
    fn name(&self) -> &'static str {
        "server.Date"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["date +'%Y-%m-%dT%H:%M:%S%z'"])
    }
    fn default(&self) -> Self::Output {
        Local::now().fixed_offset()
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(DateTime::parse_from_str(output.first().ok_or("date returned no output")?, "%Y-%m-%dT%H:%M:%S%z")?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountInfo {
    pub device: String,
    pub filesystem_type: String,
    pub options: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mounts;
impl Fact for Mounts {
    type Input = ServerKernel;
    type Output = IndexMap<String, MountInfo>;
    fn name(&self) -> &'static str {
        "server.Mounts"
    }
    fn command(&self, kernel: &ServerKernel) -> StringCommand {
        StringCommand::new([match kernel {
            ServerKernel::FreeBsd => "mount -p --libxo json",
            ServerKernel::OpenBsd => "mount -v",
            ServerKernel::Darwin | ServerKernel::Linux => "cat /proc/self/mountinfo",
        }])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], kernel: &ServerKernel) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        match kernel {
            ServerKernel::FreeBsd => parse_freebsd_mounts(output),
            ServerKernel::OpenBsd => Ok(parse_openbsd_mounts(output)),
            ServerKernel::Darwin | ServerKernel::Linux => Ok(parse_linux_mounts(output)),
        }
    }
}

fn parse_freebsd_mounts(output: &[String]) -> Result<IndexMap<String, MountInfo>, Box<dyn Error + Send + Sync>> {
    let json: Value = serde_json::from_str(&output.join("\n"))?;
    let entries = json.pointer("/mount/fstab").and_then(Value::as_array).ok_or("missing mount.fstab array")?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let path = entry.get("mntpoint")?.as_str()?.to_owned();
            Some((
                path,
                MountInfo {
                    device: entry.get("device")?.as_str()?.to_owned(),
                    filesystem_type: entry.get("fstype")?.as_str()?.to_owned(),
                    options: entry.get("opts")?.as_str()?.split(',').map(|option| option.trim().to_owned()).collect(),
                },
            ))
        })
        .collect())
}

fn parse_openbsd_mounts(output: &[String]) -> IndexMap<String, MountInfo> {
    output
        .iter()
        .filter_map(|line| {
            let (device_part, rest) = line.split_once(" on ")?;
            let (path, rest) = rest.split_once(" type ")?;
            let (filesystem_type, options) = rest.split_once(" (")?;
            let device = device_part.split_whitespace().next()?.to_owned();
            let options = options
                .strip_suffix(')')?
                .split(',')
                .map(str::trim)
                .filter(|option| !option.starts_with("ctime="))
                .map(str::to_owned)
                .collect();
            Some((path.to_owned(), MountInfo { device, filesystem_type: filesystem_type.to_owned(), options }))
        })
        .collect()
}

fn parse_linux_mounts(output: &[String]) -> IndexMap<String, MountInfo> {
    output
        .iter()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let fields = before.split_whitespace().collect::<Vec<_>>();
            let after = after.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 6 || after.len() < 3 {
                return None;
            }
            let mut options = fields[5].split(',').map(unescape_octal).collect::<Vec<_>>();
            options.extend(after[2].split(',').filter(|option| !matches!(*option, "ro" | "rw")).map(unescape_octal));
            Some((
                unescape_octal(fields[4]),
                MountInfo { device: unescape_octal(after[1]), filesystem_type: after[0].to_owned(), options },
            ))
        })
        .collect()
}

fn unescape_octal(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut result = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..=index + 3].iter().all(u8::is_ascii_digit)
        {
            let digits = &value[index + 1..index + 4];
            if let Ok(code) = u8::from_str_radix(digits, 8) {
                result.push(char::from(code));
                index += 4;
                continue;
            }
        }
        if let Some(character) = value[index..].chars().next() {
            result.push(character);
            index += character.len_utf8();
        } else {
            break;
        }
    }
    result
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KernelModuleInfo {
    Linux { size: String, instances: u64, state: String, depends: Option<Vec<String>> },
    FreeBsd { id: String, refs: String, address: String, size: String },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KernelModules;
impl Fact for KernelModules {
    type Input = ServerKernel;
    type Output = IndexMap<String, KernelModuleInfo>;
    fn name(&self) -> &'static str {
        "server.KernelModules"
    }
    fn command(&self, kernel: &ServerKernel) -> StringCommand {
        StringCommand::new([if *kernel == ServerKernel::FreeBsd {
            "kldstat | tail +2"
        } else {
            "! test -f /proc/modules || cat /proc/modules"
        }])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], kernel: &ServerKernel) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if *kernel == ServerKernel::FreeBsd {
                    (fields.len() >= 5).then(|| {
                        (
                            fields[4..].join(" "),
                            KernelModuleInfo::FreeBsd {
                                id: fields[0].to_owned(),
                                refs: fields[1].to_owned(),
                                address: fields[2].to_owned(),
                                size: fields[3].to_owned(),
                            },
                        )
                    })
                } else {
                    let instances = fields.get(2)?.parse().ok()?;
                    Some((
                        fields.first()?.to_string(),
                        KernelModuleInfo::Linux {
                            size: fields.get(1)?.to_string(),
                            instances,
                            state: fields.get(4)?.to_string(),
                            depends: (*fields.get(3)? != "-").then(|| {
                                fields[3].split(',').filter(|value| !value.is_empty()).map(str::to_owned).collect()
                            }),
                        },
                    ))
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_mounts_and_modules_match_reference_formats() {
        let date = Date.process(&["2019-01-01T00:00:00+0000".to_owned()], &()).unwrap();
        assert_eq!(date.to_rfc3339(), "2019-01-01T00:00:00+00:00");
        let mounts =
            Mounts.process(&["2 1 0:26 / / ro,noatime - ext4 /dev/sdb1 rw".to_owned()], &ServerKernel::Linux).unwrap();
        assert_eq!(mounts["/"].options, ["ro", "noatime"]);
        let escaped = Mounts
            .process(
                &[r"408 35 0:352 / /mnt/dir\040name ro shared:27 - overlay none ro,lowerdir=lower\054dir".to_owned()],
                &ServerKernel::Linux,
            )
            .unwrap();
        assert_eq!(escaped["/mnt/dir name"].options, ["ro", "lowerdir=lower,dir"]);
        let openbsd = Mounts
            .process(
                &["/dev/sd0a (disk.a) on / type ffs (rw, local, ctime=Thu Jan 1 1970)".to_owned()],
                &ServerKernel::OpenBsd,
            )
            .unwrap();
        assert_eq!(openbsd["/"].device, "/dev/sd0a");
        assert_eq!(openbsd["/"].options, ["rw", "local"]);
        let modules =
            KernelModules.process(&["libahci 32768 1 ahci, Live 0x0".to_owned()], &ServerKernel::Linux).unwrap();
        assert!(matches!(&modules["libahci"], KernelModuleInfo::Linux { instances: 1, .. }));
        let freebsd = KernelModules
            .process(&["1 46 0xffffffff80200000 1f4dad0 kernel".to_owned()], &ServerKernel::FreeBsd)
            .unwrap();
        assert!(matches!(&freebsd["kernel"], KernelModuleInfo::FreeBsd { refs, .. } if refs == "46"));
    }
}
