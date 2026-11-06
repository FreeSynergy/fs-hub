//! End-to-end: A second hub on the same socket and bad manifests are refused.

mod common;

use common::{Env, contacts_manifest};
use fs_hub::{Config, Daemon};
use fs_hub_client::{Manifest, connect_at};

#[tokio::test]
async fn a_second_hub_on_the_same_socket_is_refused_and_bad_manifests_too() {
    let env = Env::start();
    let other = tempfile::tempdir().unwrap();
    let err = Daemon::spawn(Config::new(env.socket(), other.path().join("d")))
        .err()
        .expect("must refuse");
    assert!(err.to_string().contains("already running"), "{err}");

    let bad = Manifest::new("Bad Name!", "1");
    assert!(connect_at(&env.socket(), bad).await.is_err());
    let mut old = contacts_manifest("x");
    old.contract_version = "9.9".into();
    assert!(connect_at(&env.socket(), old).await.is_err());
}
