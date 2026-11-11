//! What someone who should not get in might try.
//!
//! Every test here talks to the server the way a stranger on the network
//! would: over HTTP, with no access to the database. The server is "locked",
//! meaning it was started with an admin token, which is how a shared server
//! runs. Teams are made through the admin API, as an operator would.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use collab_server::{AppState, create_app, db};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower::ServiceExt;

const ADMIN: &str = "adm_the-real-admin-secret";

async fn locked_server() -> Router {
    let db = db::init_test_db().await.unwrap();
    let (tx, _) = broadcast::channel(256);
    create_app(AppState {
        db,
        token: Some(ADMIN.to_string()),
        audit: false,
        tx,
        sse_subscribers: Arc::new(AtomicUsize::new(0)),
        started_at: Instant::now(),
        pairing: Default::default(),
    })
}

fn request(method: &str, uri: &str, bearer: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        b = b.header("Authorization", format!("Bearer {t}"));
    }
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn send(app: &Router, req: Request<Body>) -> Response {
    app.clone().oneshot(req).await.unwrap()
}

async fn text(resp: Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn body_json(resp: Response) -> Value {
    serde_json::from_str(&text(resp).await).unwrap_or(Value::Null)
}

/// Create a team as the admin and return its member token.
async fn new_team(app: &Router, name: &str) -> String {
    let resp = send(
        app,
        request(
            "POST",
            "/admin/teams",
            Some(ADMIN),
            Some(json!({ "name": name })),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK, "creating team {name}");
    body_json(resp).await["token"].as_str().unwrap().to_string()
}

async fn pairing_code(app: &Router, member_token: &str) -> String {
    let resp = send(
        app,
        request("POST", "/pair/codes", Some(member_token), Some(json!({}))),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    body_json(resp).await["code"].as_str().unwrap().to_string()
}

async fn redeem(app: &Router, code: Value) -> Response {
    send(
        app,
        request("POST", "/pair/redeem", None, Some(json!({ "code": code }))),
    )
    .await
}

fn message(sender: &str, recipient: &str, content: &str) -> Value {
    json!({ "sender": sender, "recipient": recipient, "content": content, "refs": [] })
}

// ─── Getting in without a token ──────────────────────────────────────────────

#[tokio::test]
async fn near_miss_credentials_are_refused() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;

    let upper = member.to_uppercase();
    let truncated = &member[..member.len() - 1];
    let padded = format!("{member} ");
    let doubled = format!("{member}{member}");
    let attempts: Vec<(&str, String)> = vec![
        ("empty bearer", "Bearer ".into()),
        ("no scheme", member.clone()),
        ("basic scheme", format!("Basic {member}")),
        ("wrong case token", format!("Bearer {upper}")),
        ("one character short", format!("Bearer {truncated}")),
        ("trailing space", format!("Bearer {padded}")),
        ("token twice", format!("Bearer {doubled}")),
        (
            "admin, wrong case",
            format!("Bearer {}", ADMIN.to_uppercase()),
        ),
        (
            "admin, one short",
            format!("Bearer {}", &ADMIN[..ADMIN.len() - 1]),
        ),
        ("sql in the token", "Bearer ' OR '1'='1".into()),
        ("sql wildcard", "Bearer %".into()),
        ("null word", "Bearer null".into()),
    ];
    for (what, header) in attempts {
        let req = Request::builder()
            .method("GET")
            .uri("/roster")
            .header("Authorization", header)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            send(&app, req).await.status(),
            StatusCode::UNAUTHORIZED,
            "{what}"
        );
    }
}

#[tokio::test]
async fn a_token_in_the_address_is_checked_like_one_in_the_header() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;

    for uri in [
        "/roster?token=",
        "/roster?token=wrong",
        "/roster?token=%27%20OR%20%271%27%3D%271",
        "/roster?token=wrong&token=also-wrong",
        "/roster?nottoken=x",
    ] {
        let resp = send(&app, request("GET", uri, None, None)).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }

    // A bad header is not rescued by a good query value, or the reverse:
    // whichever the server reads, a wrong credential must not get in on
    // the strength of a guess made elsewhere in the request.
    let both_wrong = send(
        &app,
        request("GET", "/roster?token=wrong", Some("wrong"), None),
    )
    .await;
    assert_eq!(both_wrong.status(), StatusCode::UNAUTHORIZED);

    let good = send(
        &app,
        request("GET", &format!("/roster?token={member}"), None, None),
    )
    .await;
    assert_eq!(good.status(), StatusCode::OK);
}

#[tokio::test]
async fn every_data_route_refuses_a_stranger() {
    let app = locked_server().await;
    let routes: Vec<(&str, &str, Option<Value>)> = vec![
        ("POST", "/messages", Some(message("x", "y", "hi"))),
        ("GET", "/messages/y", None),
        ("GET", "/history/y", None),
        ("GET", "/roster", None),
        ("PUT", "/presence/y", Some(json!({ "role": "spy" }))),
        ("DELETE", "/presence/y", None),
        ("DELETE", "/messages/cleanup", None),
        (
            "POST",
            "/todos",
            Some(json!({ "instance": "y", "assigned_by": "x", "description": "d" })),
        ),
        ("GET", "/todos/y", None),
        ("PATCH", "/todos/abcd1234/done", None),
        (
            "POST",
            "/worker/lease",
            Some(json!({ "instance_id": "y", "pid": 1, "host": "h" })),
        ),
        ("DELETE", "/worker/lease/y", None),
        ("POST", "/usage", Some(json!({}))),
        ("GET", "/usage", None),
        ("GET", "/admin/teams", None),
        ("POST", "/admin/teams", Some(json!({ "name": "mine-now" }))),
        ("POST", "/admin/teams/any/tokens", Some(json!({}))),
        ("DELETE", "/admin/teams/any/tokens/abcd", None),
        ("POST", "/pair/codes", Some(json!({}))),
        ("GET", "/events", None),
        ("GET", "/events/y", None),
    ];
    for (method, uri, body) in routes {
        let resp = send(&app, request(method, uri, None, body)).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

// ─── A member of one team reaching into another ──────────────────────────────

#[tokio::test]
async fn a_member_cannot_do_admin_things() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;

    let list = send(&app, request("GET", "/admin/teams", Some(&member), None)).await;
    assert_ne!(list.status(), StatusCode::OK);

    let create = send(
        &app,
        request(
            "POST",
            "/admin/teams",
            Some(&member),
            Some(json!({ "name": "side-team" })),
        ),
    )
    .await;
    assert_ne!(create.status(), StatusCode::OK);

    // The admin's own list shows no team was made by the attempt.
    let teams =
        body_json(send(&app, request("GET", "/admin/teams", Some(ADMIN), None)).await).await;
    let names: Vec<&str> = teams
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, vec!["crabs"]);
}

#[tokio::test]
async fn one_team_cannot_read_or_change_another_teams_things() {
    let app = locked_server().await;
    let crabs = new_team(&app, "crabs").await;
    let rivals = new_team(&app, "rivals").await;

    // The crabs have a member, a message, a todo and a worker lease.
    for req in [
        request(
            "PUT",
            "/presence/deck",
            Some(&crabs),
            Some(json!({ "role": "builder" })),
        ),
        request(
            "POST",
            "/messages",
            Some(&crabs),
            Some(message("mac", "deck", "the launch code is 7")),
        ),
        request(
            "POST",
            "/todos",
            Some(&crabs),
            Some(json!({ "instance": "deck", "assigned_by": "mac", "description": "secret plan" })),
        ),
        request(
            "POST",
            "/worker/lease",
            Some(&crabs),
            Some(json!({ "instance_id": "deck", "pid": 4242, "host": "steamdeck" })),
        ),
    ] {
        let uri = req.uri().to_string();
        assert!(send(&app, req).await.status().is_success(), "setup {uri}");
    }
    let todos =
        body_json(send(&app, request("GET", "/todos/deck", Some(&crabs), None)).await).await;
    let todo_hash = todos[0]["hash"].as_str().unwrap().to_string();

    // The rivals know every name. They still see nothing.
    for uri in [
        "/messages/deck",
        "/history/deck",
        "/history/all",
        "/roster",
        "/todos/deck",
        "/usage",
    ] {
        let resp = send(&app, request("GET", uri, Some(&rivals), None)).await;
        let body = text(resp).await;
        for secret in ["launch code", "secret plan", "builder", "steamdeck"] {
            assert!(
                !body.contains(secret),
                "GET {uri} leaked {secret:?}: {body}"
            );
        }
    }

    // They cannot close the crabs' todo, even with its full hash.
    let done = send(
        &app,
        request(
            "PATCH",
            &format!("/todos/{todo_hash}/done"),
            Some(&rivals),
            None,
        ),
    )
    .await;
    assert_eq!(done.status(), StatusCode::NOT_FOUND);

    // They cannot remove the crabs' member or release its worker.
    send(
        &app,
        request("DELETE", "/presence/deck", Some(&rivals), None),
    )
    .await;
    send(
        &app,
        request("DELETE", "/worker/lease/deck?pid=4242", Some(&rivals), None),
    )
    .await;
    send(
        &app,
        request("DELETE", "/messages/cleanup", Some(&rivals), None),
    )
    .await;

    let roster = text(send(&app, request("GET", "/roster", Some(&crabs), None)).await).await;
    assert!(
        roster.contains("deck"),
        "the member is still there: {roster}"
    );
    let history = text(send(&app, request("GET", "/history/deck", Some(&crabs), None)).await).await;
    assert!(
        history.contains("launch code"),
        "the message is still there: {history}"
    );
    let todos =
        body_json(send(&app, request("GET", "/todos/deck", Some(&crabs), None)).await).await;
    assert_eq!(todos.as_array().unwrap().len(), 1, "the todo is still open");

    // A second worker for the same name is still refused: the lease held.
    let second = send(
        &app,
        request(
            "POST",
            "/worker/lease",
            Some(&crabs),
            Some(json!({ "instance_id": "deck", "pid": 9999, "host": "elsewhere" })),
        ),
    )
    .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_admin_token_does_not_read_team_messages() {
    // The admin runs the server; that is not the same as being in a team.
    let app = locked_server().await;
    let crabs = new_team(&app, "crabs").await;
    send(
        &app,
        request(
            "POST",
            "/messages",
            Some(&crabs),
            Some(message("mac", "deck", "team only")),
        ),
    )
    .await;

    for uri in ["/messages/deck", "/history/deck", "/roster"] {
        let body = text(send(&app, request("GET", uri, Some(ADMIN), None)).await).await;
        assert!(
            !body.contains("team only") && !body.contains("mac"),
            "GET {uri}: {body}"
        );
    }
}

// ─── Hostile input ───────────────────────────────────────────────────────────

#[tokio::test]
async fn hostile_names_and_bodies_never_crash_the_server() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;

    let long = "a".repeat(5000);
    let nasty = [
        "' OR '1'='1",
        "x'; DROP TABLE messages; --",
        "%",
        "_",
        "..%2F..%2Fetc%2Fpasswd",
        "%00",
        "<script>alert(1)</script>",
        long.as_str(),
    ];
    for name in nasty {
        let encoded: String = name
            .bytes()
            .map(|b| match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'%' => (b as char).to_string(),
                other => format!("%{other:02X}"),
            })
            .collect();
        for (method, uri) in [
            ("GET", format!("/messages/{encoded}")),
            ("GET", format!("/history/{encoded}")),
            ("GET", format!("/todos/{encoded}")),
            ("PUT", format!("/presence/{encoded}")),
            ("DELETE", format!("/presence/{encoded}")),
            ("PATCH", format!("/todos/{encoded}/done")),
            ("DELETE", format!("/worker/lease/{encoded}")),
        ] {
            let body = (method == "PUT").then(|| json!({ "role": name }));
            let resp = send(&app, request(method, &uri, Some(&member), body)).await;
            assert!(
                !resp.status().is_server_error(),
                "{method} with name {:?} gave {}",
                &name[..name.len().min(40)],
                resp.status()
            );
        }
        let resp = send(
            &app,
            request(
                "POST",
                "/messages",
                Some(&member),
                Some(message(name, name, name)),
            ),
        )
        .await;
        assert!(
            !resp.status().is_server_error(),
            "message from {:?}",
            &name[..name.len().min(40)]
        );
    }

    // The table the injection named is still there and still answers.
    let ok = send(
        &app,
        request(
            "POST",
            "/messages",
            Some(&member),
            Some(message("mac", "deck", "still here")),
        ),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let history =
        text(send(&app, request("GET", "/history/deck", Some(&member), None)).await).await;
    assert!(history.contains("still here"), "{history}");
}

#[tokio::test]
async fn a_wildcard_name_does_not_match_other_members() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    send(
        &app,
        request(
            "POST",
            "/messages",
            Some(&member),
            Some(message("mac", "deck", "for deck only")),
        ),
    )
    .await;

    for uri in [
        "/messages/%25",
        "/history/%25",
        "/messages/d%25",
        "/history/_eck",
        "/todos/%25",
    ] {
        let body = text(send(&app, request("GET", uri, Some(&member), None)).await).await;
        assert!(!body.contains("for deck only"), "GET {uri}: {body}");
    }
}

#[tokio::test]
async fn hostile_team_names_are_refused_by_the_admin_api() {
    let app = locked_server().await;
    for name in ["", " ", "a b", "../etc", "x';--", "<b>", "crabs\n"] {
        let resp = send(
            &app,
            request(
                "POST",
                "/admin/teams",
                Some(ADMIN),
                Some(json!({ "name": name })),
            ),
        )
        .await;
        assert!(
            resp.status().is_client_error(),
            "team name {name:?} gave {}",
            resp.status()
        );
    }
}

// ─── Pairing codes ───────────────────────────────────────────────────────────

#[tokio::test]
async fn malformed_redeem_requests_are_refused_without_a_token() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    let _live = pairing_code(&app, &member).await;

    let bodies: Vec<Value> = vec![
        json!({}),
        json!({ "code": null }),
        json!({ "code": 123456 }),
        json!({ "code": ["123456"] }),
        json!({ "code": { "$ne": "" } }),
        json!({ "code": "" }),
        json!({ "code": "%" }),
        json!({ "code": "______" }),
        json!({ "code": "' OR 1=1 --" }),
        json!({ "code": "9".repeat(100_000) }),
        json!("123456"),
        json!([]),
    ];
    for body in bodies {
        let label: String = body.to_string().chars().take(40).collect();
        let resp = send(&app, request("POST", "/pair/redeem", None, Some(body))).await;
        assert!(
            resp.status().is_client_error(),
            "{label} gave {}",
            resp.status()
        );
        let reply = text(resp).await;
        assert!(
            !reply.contains("tm_"),
            "{label} was handed a token: {reply}"
        );
    }

    let not_json = Request::builder()
        .method("POST")
        .uri("/pair/redeem")
        .header("content-type", "application/json")
        .body(Body::from("{\"code\": \"123"))
        .unwrap();
    assert!(send(&app, not_json).await.status().is_client_error());
}

#[tokio::test]
async fn guessing_codes_gets_nowhere() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    let real = pairing_code(&app, &member).await;

    // Walk the code space from the bottom, as a script would.
    let mut answers = std::collections::BTreeSet::new();
    for n in 0..50u32 {
        let guess = format!("{n:06}");
        if guess == real {
            continue;
        }
        let resp = redeem(&app, json!(guess)).await;
        let status = resp.status();
        answers.insert(format!("{status} {}", text(resp).await));
        assert!(status.is_client_error());
    }
    // Every refusal reads the same: nothing says "close" or "expired".
    assert_eq!(answers.len(), 1, "refusals differ: {answers:?}");

    // And the code that was live when the guessing started is gone.
    assert!(redeem(&app, json!(real)).await.status().is_client_error());
}

#[tokio::test]
async fn a_refusal_says_the_same_for_a_used_code_as_for_a_wrong_one() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    let code = pairing_code(&app, &member).await;
    assert_eq!(redeem(&app, json!(code)).await.status(), StatusCode::OK);

    let used = redeem(&app, json!(code)).await;
    let used = (used.status(), text(used).await);
    let wrong_code = if code == "000000" { "000001" } else { "000000" };
    let wrong = redeem(&app, json!(wrong_code)).await;
    let wrong = (wrong.status(), text(wrong).await);
    assert_eq!(used, wrong);
}

#[tokio::test]
async fn a_paired_machine_gets_into_its_own_team_only() {
    let app = locked_server().await;
    let crabs = new_team(&app, "crabs").await;
    let rivals = new_team(&app, "rivals").await;
    send(
        &app,
        request(
            "POST",
            "/messages",
            Some(&rivals),
            Some(message("a", "b", "rivals only")),
        ),
    )
    .await;

    let code = pairing_code(&app, &crabs).await;
    let joined = body_json(redeem(&app, json!(code)).await).await;
    assert_eq!(joined["team_name"], "crabs");
    let token = joined["token"].as_str().unwrap();

    let history = text(send(&app, request("GET", "/history/b", Some(token), None)).await).await;
    assert!(!history.contains("rivals only"), "{history}");
    let admin = send(&app, request("GET", "/admin/teams", Some(token), None)).await;
    assert_ne!(admin.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_revoked_token_stops_working_at_once() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    let teams =
        body_json(send(&app, request("GET", "/admin/teams", Some(ADMIN), None)).await).await;
    let team_id = teams[0]["team_id"].as_str().unwrap().to_string();

    // A second machine pairs in, then its token is revoked.
    let code = pairing_code(&app, &member).await;
    let joined = body_json(redeem(&app, json!(code)).await).await;
    let token = joined["token"].as_str().unwrap().to_string();
    assert_eq!(
        send(&app, request("GET", "/roster", Some(&token), None))
            .await
            .status(),
        StatusCode::OK
    );

    let prefix: String = collab_server::hash_token(&token).chars().take(12).collect();
    let revoke = send(
        &app,
        request(
            "DELETE",
            &format!("/admin/teams/{team_id}/tokens/{prefix}"),
            Some(ADMIN),
            None,
        ),
    )
    .await;
    assert!(
        revoke.status().is_success(),
        "revoke gave {}",
        revoke.status()
    );

    assert_eq!(
        send(&app, request("GET", "/roster", Some(&token), None))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // It cannot be used to mint its own replacement either.
    assert_eq!(
        send(
            &app,
            request("POST", "/pair/codes", Some(&token), Some(json!({})))
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    // The other member is untouched.
    assert_eq!(
        send(&app, request("GET", "/roster", Some(&member), None))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_revoke_wildcard_cannot_take_out_every_token() {
    let app = locked_server().await;
    let member = new_team(&app, "crabs").await;
    let teams =
        body_json(send(&app, request("GET", "/admin/teams", Some(ADMIN), None)).await).await;
    let team_id = teams[0]["team_id"].as_str().unwrap().to_string();

    // Even the admin should not be able to revoke by a pattern: a typo of
    // "%" or "_" for a prefix would lock the whole team out.
    for prefix in ["%25", "_", "%25%25%25%25", "____"] {
        send(
            &app,
            request(
                "DELETE",
                &format!("/admin/teams/{team_id}/tokens/{prefix}"),
                Some(ADMIN),
                None,
            ),
        )
        .await;
    }
    assert_eq!(
        send(&app, request("GET", "/roster", Some(&member), None))
            .await
            .status(),
        StatusCode::OK
    );
}
