//! `collab stop` must tell the server the worker is gone.
//!
//! Stopping used to kill the worker process and nothing else. The server
//! went on showing the worker as present for another 30 seconds and kept
//! its lease, so `collab start` straight afterwards refused with "these
//! workers are already heartbeating on the server". From the GUI that
//! meant Stop All followed by Start Workers always failed.
//!
//! Like `e2e_team.rs`, this runs the real HTTP server in-process on a
//! random port and drives it with the CLI's own client code. No worker
//! process is spawned.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use collab_server::{create_app, db, AppState, BroadcastMsg};
use holdmybeer_cli::client::{CollabClient, LeaseOutcome};
use holdmybeer_cli::lifecycle;
use tokio::sync::broadcast;

const ADMIN_TOKEN: &str = "admin-secret";

async fn start_server_in_process() -> String {
    let pool = db::init_test_db().await.unwrap();
    let (tx, _) = broadcast::channel::<Arc<BroadcastMsg>>(256);
    let state = AppState {
        db: pool,
        token: Some(ADMIN_TOKEN.to_string()),
        audit: false,
        tx,
        sse_subscribers: Arc::new(AtomicUsize::new(0)),
        started_at: Instant::now(),
        pairing: Default::default(),
    };
    let app = create_app(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

async fn admin_create_team(server: &str, name: &str) -> String {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/admin/teams", server))
        .header("Authorization", format!("Bearer {}", ADMIN_TOKEN))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        200,
        "admin create team must succeed"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

async fn on_roster(client: &CollabClient, name: &str) -> bool {
    client
        .fetch_roster_pub()
        .await
        .unwrap()
        .iter()
        .any(|w| w.instance_id == name)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_worker_is_gone_from_the_server_at_once() {
    let server = start_server_in_process().await;
    let token = admin_create_team(&server, "stoptest").await;

    // A running worker: it holds its lease and shows on the roster.
    let builder = CollabClient::new(&server, "builder", Some(&token));
    assert!(matches!(
        builder.acquire_lease(100, "host-a").await.unwrap(),
        LeaseOutcome::Held { .. }
    ));
    builder.heartbeat(Some("builds things")).await.unwrap();
    assert!(
        on_roster(&builder, "builder").await,
        "worker should be on the roster while running"
    );

    // While it runs, a second process may not take its place.
    assert!(matches!(
        builder.acquire_lease(200, "host-a").await.unwrap(),
        LeaseOutcome::Conflict { .. }
    ));

    // What `collab stop` does after killing the process.
    lifecycle::release_on_server(&server, "builder", 100, Some(&token))
        .await
        .expect("telling the server should succeed");

    // The server no longer lists it, so `collab start` will not refuse...
    assert!(
        !on_roster(&builder, "builder").await,
        "a stopped worker must leave the roster"
    );
    // ...and the new worker process gets the lease without waiting it out.
    match builder.acquire_lease(200, "host-a").await.unwrap() {
        LeaseOutcome::Held { taken_over } => {
            assert!(
                !taken_over,
                "the lease was released, so there is nothing to take over"
            )
        }
        other => panic!("expected the lease to be free, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_one_worker_leaves_the_others_alone() {
    let server = start_server_in_process().await;
    let token = admin_create_team(&server, "stoptest2").await;

    let builder = CollabClient::new(&server, "builder", Some(&token));
    let reviewer = CollabClient::new(&server, "reviewer", Some(&token));
    builder.acquire_lease(100, "host-a").await.unwrap();
    reviewer.acquire_lease(101, "host-a").await.unwrap();
    builder.heartbeat(Some("builds")).await.unwrap();
    reviewer.heartbeat(Some("reviews")).await.unwrap();

    lifecycle::release_on_server(&server, "builder", 100, Some(&token))
        .await
        .unwrap();

    assert!(!on_roster(&reviewer, "builder").await);
    assert!(
        on_roster(&reviewer, "reviewer").await,
        "the other worker must stay"
    );
    assert!(
        matches!(
            reviewer.acquire_lease(999, "host-a").await.unwrap(),
            LeaseOutcome::Conflict { .. }
        ),
        "the other worker must keep its lease"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_server_is_an_error_not_a_panic() {
    // Port 1 on loopback: nothing listens there. `collab stop` reports this
    // and carries on, because the process is already dead either way.
    let result = lifecycle::release_on_server("http://127.0.0.1:1", "builder", 100, None).await;
    assert!(result.is_err());
}
