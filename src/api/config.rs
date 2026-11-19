use std::mem;

use indexmap::IndexMap;

use super::{EnvValue, HiddenValue};

const DEFAULT_TEMP_DIRECTORY: &str = "/tmp";
const DEFAULT_SHELL: &str = "sh";

/// Deploy-wide defaults corresponding to pyinfra's `Config`.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub fail_percent: Option<u8>,
    pub connect_timeout_seconds: u64,
    pub temp_directory: Option<String>,
    pub default_temp_directory: String,
    pub parallel: usize,
    pub require_pyinfra_version: Option<String>,
    pub require_packages: Option<String>,
    pub su_user: Option<String>,
    pub use_su_login: bool,
    pub su_shell: Option<String>,
    pub preserve_su_env: bool,
    pub su_password: Option<HiddenValue>,
    pub sudo: bool,
    pub sudo_user: Option<String>,
    pub preserve_sudo_env: bool,
    pub sudo_set_home: bool,
    pub use_sudo_login: bool,
    pub sudo_password: Option<HiddenValue>,
    pub use_sudo_password: bool,
    pub doas: bool,
    pub doas_user: Option<String>,
    pub dzdo: bool,
    pub dzdo_user: Option<String>,
    pub ignore_errors: bool,
    pub shell: String,
    pub diff: bool,
    pub retry: usize,
    pub retry_delay_seconds: u64,
    pub inherit_env: Vec<String>,
    pub env: IndexMap<String, EnvValue>,
    locked_state: Option<Box<ConfigState>>,
}

/// A restorable snapshot of all pyinfra configuration defaults.
///
/// Like pyinfra, this deliberately excludes `env`, which is applied separately from the default
/// configuration keys.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigState {
    config: Config,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fail_percent: None,
            connect_timeout_seconds: 10,
            temp_directory: None,
            default_temp_directory: DEFAULT_TEMP_DIRECTORY.to_owned(),
            parallel: 0,
            require_pyinfra_version: None,
            require_packages: None,
            su_user: None,
            use_su_login: false,
            su_shell: None,
            preserve_su_env: false,
            su_password: None,
            sudo: false,
            sudo_user: None,
            preserve_sudo_env: false,
            sudo_set_home: true,
            use_sudo_login: false,
            sudo_password: None,
            use_sudo_password: false,
            doas: false,
            doas_user: None,
            dzdo: false,
            dzdo_user: None,
            ignore_errors: false,
            shell: DEFAULT_SHELL.to_owned(),
            diff: false,
            retry: 0,
            retry_delay_seconds: 5,
            inherit_env: Vec::new(),
            env: IndexMap::new(),
            locked_state: None,
        }
    }
}

impl Config {
    /// Captures the fields represented by pyinfra's `ConfigDefaults`.
    #[must_use]
    pub fn current_state(&self) -> ConfigState {
        let mut config = self.clone();
        config.env.clear();
        config.locked_state = None;
        ConfigState { config }
    }

    /// Restores fields represented by pyinfra's `ConfigDefaults` without changing `env`.
    pub fn set_current_state(&mut self, state: &ConfigState) {
        let env = mem::take(&mut self.env);
        let locked_state = self.locked_state.take();
        *self = state.config.clone();
        self.env = env;
        self.locked_state = locked_state;
    }

    /// Captures the state that `reset_locked_state` will later restore.
    pub fn lock_current_state(&mut self) {
        self.locked_state = Some(Box::new(self.current_state()));
    }

    /// Restores the last locked state, returning `false` if no state was locked.
    pub fn reset_locked_state(&mut self) -> bool {
        let Some(state) = self.locked_state.clone() else {
            return false;
        };
        self.set_current_state(&state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, DEFAULT_SHELL, DEFAULT_TEMP_DIRECTORY};

    #[test]
    fn defaults_match_pyinfra_config_defaults() {
        let config = Config::default();

        assert_eq!(config.connect_timeout_seconds, 10);
        assert_eq!(config.default_temp_directory, DEFAULT_TEMP_DIRECTORY);
        assert_eq!(config.shell, DEFAULT_SHELL);
        assert!(config.sudo_set_home);
        assert_eq!(config.retry_delay_seconds, 5);
        assert_eq!(config.fail_percent, None);
        assert_eq!(config.parallel, 0);
    }

    #[test]
    fn reset_locked_state_restores_defaults_but_preserves_environment() {
        let mut config = Config::default();
        config.sudo = true;
        config.env.insert("FIRST".to_owned(), "one".into());
        config.lock_current_state();

        config.sudo = false;
        config.retry = 3;
        config.env.insert("SECOND".to_owned(), "two".into());

        assert!(config.reset_locked_state());
        assert!(config.sudo);
        assert_eq!(config.retry, 0);
        assert_eq!(config.env.len(), 2);
    }
}
