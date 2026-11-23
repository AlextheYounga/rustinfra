use indexmap::IndexMap;

pub type PackageMap = IndexMap<String, Vec<String>>;

pub fn insert_package(packages: &mut PackageMap, name: &str, version: &str) {
    packages.entry(name.to_owned()).or_default().push(version.to_owned());
}

pub type Repository = IndexMap<String, String>;

pub fn parse_repositories(output: &[String]) -> Vec<Repository> {
    let mut repositories = Vec::new();
    let mut current = Repository::new();
    let mut filename = None::<String>;
    for line in output.iter().map(|line| line.trim()).filter(|line| !line.is_empty()) {
        if let Some(path) = line.strip_prefix("#pyinfra-filename:") {
            if !current.is_empty() {
                repositories.push(current);
                current = Repository::new();
            }
            filename = Some(path.trim().to_owned()).filter(|path| !path.is_empty());
        } else if line.starts_with('#') {
            continue;
        } else if line.starts_with('[') && line.ends_with(']') {
            if !current.is_empty() {
                repositories.push(current);
                current = Repository::new();
            }
            let id = line[1..line.len() - 1].to_owned();
            current.insert("repoid".to_owned(), id.clone());
            current.insert("name".to_owned(), id);
            if let Some(filename) = &filename {
                current.insert("filename".to_owned(), filename.clone());
            }
        } else if !current.is_empty() {
            if let Some((key, value)) = line.split_once('=') {
                current.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
    }
    if !current.is_empty() {
        repositories.push(current);
    }
    repositories
}
