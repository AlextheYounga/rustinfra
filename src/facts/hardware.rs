use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

#[derive(Clone, Debug, PartialEq)]
pub enum CpuValue {
    Text(String),
    Flags(Vec<String>),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuInfo;

impl Fact for CpuInfo {
    type Input = ();
    type Output = Option<IndexMap<String, CpuValue>>;

    fn name(&self) -> &'static str {
        "hardware.CpuInfo"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["LANG=C lscpu"])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("lscpu")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        if output.is_empty() {
            return Ok(None);
        }
        let mut info = IndexMap::new();
        for line in output {
            let Some((key, value)) = line.split_once(':') else { continue };
            let key = key.trim().to_owned();
            let value = if key.to_ascii_lowercase().contains("flag") {
                CpuValue::Flags(value.split_whitespace().map(str::to_owned).collect())
            } else {
                CpuValue::Text(value.trim().to_owned())
            };
            info.insert(key, value);
        }
        Ok((!info.is_empty()).then_some(info))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Cpus;

impl Fact for Cpus {
    type Input = ();
    type Output = Option<u32>;
    fn name(&self) -> &'static str {
        "hardware.Cpus"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["getconf NPROCESSORS_ONLN 2> /dev/null || getconf _NPROCESSORS_ONLN"])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output.first().and_then(|value| value.parse().ok()))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Memory;

impl Fact for Memory {
    type Input = ();
    type Output = Option<u64>;
    fn name(&self) -> &'static str {
        "hardware.Memory"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["LANG=C vmstat -s"])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("vmstat")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut data = IndexMap::new();
        for line in output {
            let Some((value, key)) = line.trim().split_once(' ') else { continue };
            if let Ok(value) = value.parse::<u64>() {
                data.insert(key.trim(), value);
            }
        }
        let mut total_kib = data.get("K total memory").or_else(|| data.get("total memory")).copied();
        if total_kib.is_none_or(|value| value == 0) {
            let bytes_per_page = data.get("bytes per page").copied();
            let pages = data.get("pages managed").copied().or_else(|| {
                let total =
                    ["pages active", "pages inactive", "pages wired down", "pages free", "pages in the laundry queue"]
                        .iter()
                        .map(|key| data.get(key).copied().unwrap_or_default())
                        .sum::<u64>();
                (total > 0).then_some(total)
            });
            total_kib =
                bytes_per_page.zip(pages).and_then(|(bytes, pages)| bytes.checked_mul(pages)).map(|bytes| bytes / 1024);
        }
        Ok(total_kib.filter(|value| *value > 0).map(|value| (value + 512) / 1024))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDevice {
    pub blocks: String,
    pub used: String,
    pub available: String,
    pub used_percent: String,
    pub mount: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BlockDevices;

impl Fact for BlockDevices {
    type Input = ();
    type Output = IndexMap<String, BlockDevice>;
    fn name(&self) -> &'static str {
        "hardware.BlockDevices"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["df"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() != 6 || fields[0] == "none" || !fields[4].ends_with('%') {
                    return None;
                }
                Some((
                    fields[0].to_owned(),
                    BlockDevice {
                        blocks: fields[1].to_owned(),
                        used: fields[2].to_owned(),
                        available: fields[3].to_owned(),
                        used_percent: fields[4].trim_end_matches('%').to_owned(),
                        mount: fields[5].to_owned(),
                    },
                ))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockDevices, CpuInfo, CpuValue, Cpus, Fact, Memory};

    #[test]
    fn cpu_facts_parse_text_flags_and_counts() {
        let info =
            CpuInfo.process(&["Architecture: x86_64".to_owned(), "Flags: fpu sse2".to_owned()], &()).unwrap().unwrap();
        assert_eq!(info["Architecture"], CpuValue::Text("x86_64".to_owned()));
        assert_eq!(info["Flags"], CpuValue::Flags(vec!["fpu".to_owned(), "sse2".to_owned()]));
        assert_eq!(Cpus.process(&["4".to_owned()], &()).unwrap(), Some(4));
        assert_eq!(Cpus.process(&["unknown".to_owned()], &()).unwrap(), None);
    }

    #[test]
    fn memory_supports_linux_bsd_and_freebsd_shapes() {
        assert_eq!(Memory.process(&["131766272 K total memory".to_owned()], &()).unwrap(), Some(128_678));
        assert_eq!(
            Memory.process(&["4096 bytes per page".to_owned(), "500000 pages managed".to_owned()], &()).unwrap(),
            Some(1953)
        );
        assert_eq!(
            Memory
                .process(
                    &["4096 bytes per page".to_owned(), "50000 pages active".to_owned(), "8640 pages free".to_owned()],
                    &()
                )
                .unwrap(),
            Some(229)
        );
    }

    #[test]
    fn block_devices_skip_headers_and_virtual_none_devices() {
        let devices = BlockDevices
            .process(
                &[
                    "Filesystem 512-blocks Used Available Capacity Mounted on".to_owned(),
                    "/dev/disk1s1 500 100 400 36% /".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(devices["/dev/disk1s1"].used_percent, "36");
    }
}
