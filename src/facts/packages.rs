use indexmap::IndexMap;

pub type PackageMap = IndexMap<String, Vec<String>>;

pub fn insert_package(packages: &mut PackageMap, name: &str, version: &str) {
    packages.entry(name.to_owned()).or_default().push(version.to_owned());
}
