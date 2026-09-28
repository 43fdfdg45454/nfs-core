//! Every way into a mutual-TLS export that must fail (NFS_TLS_REFUSALS, over TCP): no TLS, no
//! client certificate, a certificate of another CA, and a server name the certificate lacks.

use nfs_testkit as common;

use nfs_client::{Client, Security};

async fn refused(security: Security) -> String {
    let mut config = common::config().unwrap();
    config.security = security;
    match Client::connect(config, "/mtls").await {
        Ok(_) => panic!("a client got into the mutual-TLS export"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn the_mutual_tls_export_refuses() {
    if std::env::var("NFS_TLS_REFUSALS").is_err() {
        return;
    }
    let host = common::host().unwrap();
    let why = [
        refused(Security::None).await,
        refused(common::tls(&host, None)).await,
        refused(common::tls(&host, Some("rogue-client"))).await,
        refused(common::tls("wrong.example.net", Some("client"))).await,
    ];
    println!("refusals: {why:#?}");
    assert!(why[3].contains("certificate"), "{}", why[3]);
}
