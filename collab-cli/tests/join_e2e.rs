//! `collab pair` and `collab join` against a real server.
//!
//! Runs the HTTP server in-process on a random port, like `e2e_team.rs`, and
//! drives it through the same functions the two commands call.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use collab_server::{create_app, db, AppState, BroadcastMsg};
use holdmybeer_cli::client::CollabClient;
use holdmybeer_cli::join;
use tempfile::TempDir;
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
    let resp = reqwest::Client::new()
        .post(format!("{}/admin/teams", server))
        .header("Authorization", format!("Bearer {}", ADMIN_TOKEN))
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_pairs_and_a_new_machine_joins() {
    let server = start_server_in_process().await;
    let member_token = admin_create_team(&server, "crabs").await;

    // On a machine already in the team: `collab pair`.
    let pair = join::request_code(&server, &member_token).await.unwrap();
    assert_eq!(pair.code.len(), 6);
    assert!(pair.code.chars().all(|c| c.is_ascii_digit()));
    assert_eq!(pair.expires_in, 300);

    // On the new machine: `collab join`, typed the way people type codes.
    let spaced = format!("{} {}", &pair.code[..3], &pair.code[3..]);
    let joined = join::redeem(&server, &spaced).await.unwrap();
    assert_eq!(joined.team_name, "crabs");
    assert!(joined.token.starts_with("tm_"));
    assert_ne!(
        joined.token, member_token,
        "the new machine gets its own token"
    );

    // The new token is a working team token.
    let client = CollabClient::new(&server, "new-machine", Some(&joined.token));
    client.heartbeat(Some("just joined")).await.unwrap();
    let roster = client.fetch_roster_pub().await.unwrap();
    assert!(roster.iter().any(|w| w.instance_id == "new-machine"));

    // And it is saved without disturbing what was in the file.
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("nested").join(".collab.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "# mine\ninstance = \"deck\"\nhost = \"http://old:8000\"\n",
    )
    .unwrap();
    join::write_joined_config(&path, &server, &joined, None).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("# mine"), "{saved}");
    assert!(saved.contains("instance = \"deck\""), "{saved}");
    assert!(saved.contains(&format!("host = \"{server}\"")), "{saved}");
    assert!(!saved.contains("http://old:8000"), "{saved}");
    assert!(
        saved.contains(&format!("token = \"{}\"", joined.token)),
        "{saved}"
    );
    assert!(saved.contains("team = \"crabs\""), "{saved}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_code_cannot_be_redeemed_twice() {
    let server = start_server_in_process().await;
    let member_token = admin_create_team(&server, "crabs").await;
    let pair = join::request_code(&server, &member_token).await.unwrap();

    join::redeem(&server, &pair.code).await.unwrap();
    let err = join::redeem(&server, &pair.code)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not valid"), "{err}");
    assert!(
        err.contains("collab pair"),
        "the message should say how to get a new code: {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_malformed_code_is_refused_without_asking_the_server() {
    // Nothing listens on port 1, so reaching the network would be a
    // different error.
    let err = join::redeem("http://127.0.0.1:1", "12345")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("six digits"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_server_is_explained() {
    let err = join::redeem("http://127.0.0.1:1", "482913")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("Can't reach http://127.0.0.1:1"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_needs_a_team_token() {
    let server = start_server_in_process().await;
    admin_create_team(&server, "crabs").await;

    let wrong = join::request_code(&server, "not-a-token")
        .await
        .unwrap_err()
        .to_string();
    assert!(wrong.contains("did not accept"), "{wrong}");

    let admin = join::request_code(&server, ADMIN_TOKEN)
        .await
        .unwrap_err()
        .to_string();
    assert!(admin.contains("admin token"), "{admin}");
}

#[test]
fn a_join_that_would_be_overridden_says_so() {
    let server = "http://raspi:8000";
    let env = |k: &str| match k {
        "COLLAB_SERVER" => Some("http://localhost:8000".to_string()),
        "COLLAB_TOKEN" => Some("old".to_string()),
        _ => None,
    };
    let local = std::path::Path::new("/home/me/code/project/.collab.toml");
    let warnings = join::override_warnings(env, server, Some(local));

    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings[0].contains("COLLAB_SERVER") && warnings[0].contains("http://localhost:8000"));
    assert!(warnings[1].contains("COLLAB_TOKEN"));
    assert!(warnings[2].contains("project"));
}

#[test]
fn a_clean_environment_gets_no_warnings() {
    // COLLAB_SERVER pointing at the same server is not a conflict.
    let env = |k: &str| (k == "COLLAB_SERVER").then(|| "http://raspi:8000/".to_string());
    assert!(join::override_warnings(env, "raspi:8000", None).is_empty());
}
