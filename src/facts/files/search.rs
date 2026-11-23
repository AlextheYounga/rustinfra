use std::error::Error;

use crate::api::{CommandBit, StringCommand};
use crate::facts::Fact;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FindInput {
    pub path: String,
    pub size: Option<u64>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub max_depth: Option<u64>,
    pub name: Option<String>,
    pub insensitive_name: Option<String>,
    pub regex: Option<String>,
    pub path_match: Option<String>,
    pub args: Vec<CommandBit>,
}

impl FindInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), ..Self::default() }
    }
}

fn find_command(input: &FindInput, path_type: &str) -> StringCommand {
    let mut bits = vec![
        CommandBit::from("find"),
        CommandBit::quoted(input.path.clone()),
        CommandBit::from("-type"),
        CommandBit::from(path_type),
    ];
    let has = |flag: &str| input.args.iter().any(|arg| matches!(arg, CommandBit::String(value) if value == flag));
    if !has("-size") {
        if let Some(size) = input.min_size {
            bits.extend([CommandBit::from("-size"), CommandBit::from(format!("+{size}c"))]);
        }
        if let Some(size) = input.max_size {
            bits.extend([CommandBit::from("-size"), CommandBit::from(format!("-{size}c"))]);
        }
        if let Some(size) = input.size {
            bits.extend([CommandBit::from("-size"), CommandBit::from(format!("{size}c"))]);
        }
    }
    if !has("-maxdepth")
        && let Some(depth) = input.max_depth
    {
        bits.extend([CommandBit::from("-maxdepth"), CommandBit::from(depth)]);
    }
    for (flag, value) in [
        ("-name", input.name.as_ref()),
        ("-iname", input.insensitive_name.as_ref()),
        ("-regex", input.regex.as_ref()),
        ("-path", input.path_match.as_ref()),
    ] {
        if !has(flag)
            && let Some(value) = value
        {
            bits.extend([CommandBit::from(flag), CommandBit::quoted(value.clone())]);
        }
    }
    bits.extend(input.args.iter().cloned());
    bits.push(CommandBit::from("|| true"));
    StringCommand::new(bits)
}

macro_rules! find_fact {
    ($name:ident, $fact_name:literal, $path_type:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = FindInput;
            type Output = Vec<String>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, input: &FindInput) -> StringCommand {
                find_command(input, $path_type)
            }
            fn default(&self) -> Self::Output {
                Vec::new()
            }
            fn process(&self, output: &[String], _: &FindInput) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                Ok(output.to_vec())
            }
        }
    };
}

find_fact!(FindFiles, "files.FindFiles", "f");
find_fact!(FindDirectories, "files.FindDirectories", "d");

#[derive(Clone, Copy, Debug, Default)]
pub struct Flags;
impl Fact for Flags {
    type Input = String;
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "files.Flags"
    }
    fn command(&self, path: &String) -> StringCommand {
        let path = StringCommand::new([CommandBit::quoted(path.clone())]).raw_value();
        StringCommand::new([format!("! test -e {path} || stat -f %Sf {path}")])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("chflags")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(if output.len() == 1 {
            output[0].split(',').filter(|flag| !flag.is_empty()).map(str::to_owned).collect()
        } else {
            Vec::new()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockInput {
    pub path: String,
    pub marker: String,
    pub begin: String,
    pub end: String,
}

impl BlockInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            marker: "# {mark} PYINFRA BLOCK".to_owned(),
            begin: "BEGIN".to_owned(),
            end: "END".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Block;
impl Fact for Block {
    type Input = BlockInput;
    type Output = Option<Vec<String>>;
    fn name(&self) -> &'static str {
        "files.Block"
    }
    fn command(&self, input: &BlockInput) -> StringCommand {
        let start = input.marker.replace("{mark}", &input.begin);
        let end = input.marker.replace("{mark}", &input.end);
        let path = StringCommand::new([CommandBit::quoted(input.path.clone())]).raw_value();
        let exists = StringCommand::new([CommandBit::quoted(format!("__pyinfra_exists_{}", input.path))]).raw_value();
        let missing = StringCommand::new([CommandBit::quoted(format!("__pyinfra_missing_{}", input.path))]).raw_value();
        let program =
            StringCommand::new([CommandBit::quoted(format!("/{end}/{{ f=0}} f; /{start}/{{ f=1}} "))]).raw_value();
        StringCommand::new([format!(
            "awk {program} {path} || (find {path} -type f > /dev/null && echo {exists} || echo {missing} )"
        )])
    }
    fn default(&self) -> Self::Output {
        Some(Vec::new())
    }
    fn process(&self, output: &[String], input: &BlockInput) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let first = output.first().map(String::as_str);
        if first == Some(format!("__pyinfra_missing_{}", input.path).as_str()) {
            return Ok(None);
        }
        if first == Some(format!("__pyinfra_exists_{}", input.path).as_str()) {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(output.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_flags_and_blocks_match_reference_boundaries() {
        let mut find = FindInput::new("my dir");
        find.name = Some("myfile*".to_owned());
        assert_eq!(FindFiles.command(&find).raw_value(), "find 'my dir' -type f -name 'myfile*' || true");
        assert_eq!(FindDirectories.command(&FindInput::new("mydir")).raw_value(), "find mydir -type d || true");
        assert_eq!(Flags.process(&["oneflag,twoflag".to_owned()], &String::new()).unwrap(), ["oneflag", "twoflag"]);
        let block = BlockInput::new("/tmp/foo");
        assert_eq!(
            Block.command(&block).raw_value(),
            "awk '/# END PYINFRA BLOCK/{ f=0} f; /# BEGIN PYINFRA BLOCK/{ f=1} ' /tmp/foo || (find /tmp/foo -type f > /dev/null && echo __pyinfra_exists_/tmp/foo || echo __pyinfra_missing_/tmp/foo )"
        );
        assert_eq!(Block.process(&["__pyinfra_exists_/tmp/foo".to_owned()], &block).unwrap(), Some(Vec::new()));
        assert_eq!(Block.process(&["__pyinfra_missing_/tmp/foo".to_owned()], &block).unwrap(), None);
    }
}
