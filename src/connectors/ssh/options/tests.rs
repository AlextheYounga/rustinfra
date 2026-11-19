use std::fs;
use std::time::Duration;

use serde_json::json;
use tempfile::tempdir;

use super::{HostKeyPolicy, SshConnectionOptions, SshOptionsError};
use crate::api::Data;

#[test]
fn inventory_data_overrides_ssh_defaults_and_secrets_stay_masked() {
    let data = [
        ("ssh_hostname".to_owned(), json!("target")),
        ("ssh_port".to_owned(), json!(2202)),
        ("ssh_user".to_owned(), json!("deploy")),
        ("ssh_password".to_owned(), json!("secret")),
        ("ssh_allow_agent".to_owned(), json!(false)),
        ("ssh_strict_host_key_checking".to_owned(), json!("yes")),
    ]
    .into_iter()
    .collect::<Data>();

    let options = SshConnectionOptions::from_data("alias", &data, Duration::from_secs(7)).unwrap();

    assert_eq!(options.hostname, "target");
    assert_eq!(options.port, 2202);
    assert_eq!(options.user, "deploy");
    assert_eq!(options.password.as_ref().unwrap().unmask(), "secret");
    assert_eq!(format!("{:?}", options.password), "Some(HiddenValue(\"*MASKED*\"))");
    assert!(!options.allow_agent);
    assert_eq!(options.host_key_policy, HostKeyPolicy::Strict);
    assert_eq!(options.connect_timeout, Duration::from_secs(7));
}

#[test]
fn invalid_policy_is_rejected_before_connecting() {
    let data =
        [("ssh_user".to_owned(), json!("deploy")), ("ssh_strict_host_key_checking".to_owned(), json!("sometimes"))]
            .into_iter()
            .collect::<Data>();

    let error = SshConnectionOptions::from_data("host", &data, Duration::from_secs(10)).unwrap_err();

    assert_eq!(error, SshOptionsError::InvalidHostKeyPolicy("sometimes".to_owned()));
}

#[test]
fn ssh_config_resolves_alias_and_inventory_values_override_connection_fields() {
    let directory = tempdir().unwrap();
    let config = directory.path().join("config");
    fs::write(
        &config,
        "Host alias\n  HostName config-host\n  User config-user\n  Port 2202\n  IdentityFile /config/key-one\n  IdentityFile /config/key-two\n  CertificateFile /config/key-cert.pub\n  ServerAliveInterval 15\n  StrictHostKeyChecking yes\n  UserKnownHostsFile ~/known-one ~/known-two\n  IdentityAgent ~/agent.sock\n",
    )
    .unwrap();
    let data = [
        ("ssh_config_file".to_owned(), json!(config)),
        ("ssh_user".to_owned(), json!("inventory-user")),
        ("ssh_port".to_owned(), json!(2222)),
        ("ssh_key".to_owned(), json!("/inventory/key")),
    ]
    .into_iter()
    .collect::<Data>();

    let options = SshConnectionOptions::from_data("alias", &data, Duration::from_secs(7)).unwrap();

    assert_eq!(options.hostname, "config-host");
    assert_eq!(options.user, "inventory-user");
    assert_eq!(options.port, 2222);
    assert_eq!(options.key, Some("/inventory/key".into()));
    assert!(options.config_identity_files.is_empty());
    assert_eq!(options.config_certificate_file, None);
    assert_eq!(options.server_alive_interval, Some(Duration::from_secs(15)));
    assert_eq!(options.host_key_policy, HostKeyPolicy::Strict);
    assert_eq!(options.known_hosts_files.len(), 2);
    assert!(options.known_hosts_files[0].ends_with("known-one"));
    assert!(options.identity_agent.as_ref().is_some_and(|path| path.ends_with("agent.sock")));

    let config_only_data = [("ssh_config_file".to_owned(), json!(config))].into_iter().collect::<Data>();
    let config_only = SshConnectionOptions::from_data("alias", &config_only_data, Duration::from_secs(7)).unwrap();
    assert_eq!(config_only.config_identity_files.len(), 2);
    assert_eq!(config_only.config_certificate_file, Some("/config/key-cert.pub".into()));
}

#[test]
fn proxy_jump_chain_resolves_shorthand_nested_hops_and_timeouts() {
    let directory = tempdir().unwrap();
    let config = directory.path().join("config");
    fs::write(
        &config,
        "Host first\n  HostName first.example\n  User first-user\n  ConnectTimeout 3\nHost hop\n  HostName hop.example\n  User hop-user\n  ProxyJump first\n  ConnectTimeout 5\nHost target\n  HostName target.example\n  ProxyJump override@hop:2202\n",
    )
    .unwrap();
    let data = [("ssh_config_file".to_owned(), json!(config))].into_iter().collect::<Data>();

    let options = SshConnectionOptions::from_data("target", &data, Duration::from_secs(10)).unwrap();

    assert_eq!(options.proxy_jump.len(), 2);
    assert_eq!(options.proxy_jump[0].hostname, "first.example");
    assert_eq!(options.proxy_jump[0].connect_timeout, Duration::from_secs(3));
    assert_eq!(options.proxy_jump[0].proxy_jump_channel_timeout, Duration::from_secs(5));
    assert_eq!(options.proxy_jump[1].hostname, "hop.example");
    assert_eq!(options.proxy_jump[1].user, "override");
    assert_eq!(options.proxy_jump[1].port, 2202);
    assert_eq!(options.proxy_jump[1].connect_timeout, Duration::from_secs(5));
    assert_eq!(options.proxy_jump[1].proxy_jump_channel_timeout, Duration::from_secs(10));
}

#[test]
fn direct_multi_hop_jump_uses_target_timeout_for_every_channel() {
    let directory = tempdir().unwrap();
    let config = directory.path().join("config");
    fs::write(
        &config,
        "Host first\n  ConnectTimeout 3\nHost second\n  ConnectTimeout 5\nHost target\n  ProxyJump first,second\n",
    )
    .unwrap();
    let data = [("ssh_config_file".to_owned(), json!(config))].into_iter().collect::<Data>();

    let options = SshConnectionOptions::from_data("target", &data, Duration::from_secs(10)).unwrap();

    assert_eq!(options.proxy_jump[0].connect_timeout, Duration::from_secs(3));
    assert_eq!(options.proxy_jump[1].connect_timeout, Duration::from_secs(5));
    assert!(options.proxy_jump.iter().all(|hop| hop.proxy_jump_channel_timeout == Duration::from_secs(10)));
}

#[test]
fn proxy_command_takes_precedence_and_proxy_jump_none_disables_jump() {
    let directory = tempdir().unwrap();
    let config = directory.path().join("config");
    fs::write(
        &config,
        "Host command\n  ProxyCommand relay --host %h --port %p\n  ProxyJump ignored\nHost direct\n  ProxyJump none\n",
    )
    .unwrap();
    let data = [("ssh_config_file".to_owned(), json!(config))].into_iter().collect::<Data>();

    let command = SshConnectionOptions::from_data("command", &data, Duration::from_secs(10)).unwrap();
    let direct = SshConnectionOptions::from_data("direct", &data, Duration::from_secs(10)).unwrap();

    assert_eq!(
        command.proxy_command,
        Some(vec!["relay".to_owned(), "--host".to_owned(), "%h".to_owned(), "--port".to_owned(), "%p".to_owned(),])
    );
    assert!(command.proxy_jump.is_empty());
    assert_eq!(direct.proxy_command, None);
    assert!(direct.proxy_jump.is_empty());
}
