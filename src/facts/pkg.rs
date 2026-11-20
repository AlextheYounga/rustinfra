use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Copy, Debug, Default)]
pub struct PkgPackages;

impl Fact for PkgPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "pkg.PkgPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["pkg info || pkg_info || true"])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        for line in output {
            let Some((name, version)) = line.rsplit_once('-') else {
                continue;
            };
            if !name.is_empty()
                && !version.is_empty()
                && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+'))
                && version.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
            {
                insert_package(&mut packages, name, version);
            }
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, PkgPackages};

    #[test]
    fn parses_reference_package_listing() {
        let packages = PkgPackages.process(&["nano-3.48".to_owned()], &()).unwrap();
        assert_eq!(packages["nano"], ["3.48"]);
        assert_eq!(PkgPackages.command(&()).raw_value(), "pkg info || pkg_info || true");
    }
}
