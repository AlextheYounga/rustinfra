use std::fs;

use russh::client::Handler;
use russh::keys::{Algorithm, PrivateKey, PublicKeyOrCertificate};
use tempfile::tempdir;

use super::{HandlerError, HostKeyPolicy, ServerKeyHandler};

#[tokio::test]
async fn accept_new_records_unknown_keys_but_rejects_changed_keys() {
    let directory = tempdir().unwrap();
    let known_hosts = directory.path().join("known_hosts");
    let first = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let changed = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let mut handler = ServerKeyHandler {
        hostname: "host.example".to_owned(),
        port: 22,
        known_hosts_file: Some(known_hosts.clone()),
        policy: HostKeyPolicy::AcceptNew,
    };

    let accepted = handler.check_server_key(&PublicKeyOrCertificate::from(first.public_key().clone())).await.unwrap();

    assert!(accepted);
    assert!(handler.check_known_host(first.public_key()).unwrap());
    let known_hosts_before_change = fs::read_to_string(&known_hosts).unwrap();
    let error =
        handler.check_server_key(&PublicKeyOrCertificate::from(changed.public_key().clone())).await.unwrap_err();
    assert!(matches!(error, HandlerError::HostKey(_)));
    assert_eq!(fs::read_to_string(known_hosts).unwrap(), known_hosts_before_change);
}
