use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

const PACKAGES_COMMAND: &str = "BINDIR=$(go env GOBIN); [ -z \"$BINDIR\" ] && BINDIR=\"$(go env GOPATH)/bin\"; ! test -d \"$BINDIR\" || find \"$BINDIR\" -maxdepth 1 -type f -perm -u+x -exec go version -m {} + 2>/dev/null";

#[derive(Clone, Copy, Debug, Default)]
pub struct GoPackages;

impl Fact for GoPackages {
    type Input = ();
    type Output = PackageMap;

    fn name(&self) -> &'static str {
        "go.GoPackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([PACKAGES_COMMAND])
    }

    fn default(&self) -> Self::Output {
        PackageMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("go")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = PackageMap::new();
        let mut current_path = None;
        for line in output {
            if !line.starts_with('\t') {
                current_path = None;
                continue;
            }
            let fields = line.split('\t').collect::<Vec<_>>();
            match fields.as_slice() {
                [_, "path", path, ..] => current_path = Some(*path),
                [_, "mod", _, version, ..] if current_path.is_some() => {
                    if *version != "(devel)"
                        && let Some(path) = current_path
                    {
                        insert_package(&mut packages, path, version);
                    }
                    current_path = None;
                }
                _ => {}
            }
        }
        Ok(packages)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fact, GoPackages, PACKAGES_COMMAND};

    #[test]
    fn parses_reference_build_metadata_and_skips_development_builds() {
        let packages = GoPackages
            .process(
                &[
                    "/home/user/go/bin/bitmagnet: go1.22.0".to_owned(),
                    "\tpath\tgithub.com/bitmagnet-io/bitmagnet".to_owned(),
                    "\tmod\tgithub.com/bitmagnet-io/bitmagnet\tv0.9.0\th1:aaaa".to_owned(),
                    "/home/user/go/bin/devbin: go1.22.0".to_owned(),
                    "\tpath\tcommand-line-arguments".to_owned(),
                    "\tmod\tgithub.com/example/dev\t(devel)".to_owned(),
                ],
                &(),
            )
            .unwrap();

        assert_eq!(packages["github.com/bitmagnet-io/bitmagnet"], ["v0.9.0"]);
        assert!(!packages.contains_key("command-line-arguments"));
        assert_eq!(GoPackages.command(&()).raw_value(), PACKAGES_COMMAND);
    }
}
