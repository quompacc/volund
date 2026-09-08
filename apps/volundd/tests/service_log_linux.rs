#![cfg(target_os = "linux")]
mod support;

use serde_json::{Value, json};
use std::{fs, process::Stdio, time::Duration};
use tokio::process::Command;
use volundd::session::{self, LoginInput, LoginSession, SessionPolicy};

struct HttpResult {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

async fn http(
    address: &str,
    method: &str,
    path: &str,
    body: Value,
    actor: Option<&LoginSession>,
    request_id: &str,
) -> HttpResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let payload = if method == "GET" {
        String::new()
    } else {
        body.to_string()
    };
    let mut request = format!(
        "{method} {path} HTTP/1.0\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Request-Id: {request_id}\r\n",
        payload.len()
    );
    if let Some(actor) = actor {
        request.push_str(&format!(
            "Cookie: volund_session={}\r\nX-CSRF-Token: {}\r\n",
            actor.session_token, actor.csrf_token
        ));
    }
    request.push_str("\r\n");
    request.push_str(&payload);
    let output = tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut output = Vec::new();
        stream
            .take(5 * 1024 * 1024)
            .read_to_end(&mut output)
            .await
            .unwrap();
        output
    })
    .await
    .unwrap();
    let boundary = output
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8(output[..boundary].to_vec()).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    HttpResult {
        status,
        headers,
        body: output[boundary + 4..].to_vec(),
    }
}

#[tokio::test]
async fn real_daemon_logs_and_error_responses_redact_credentials_and_correlate_failures() {
    let Some(db) = support::test_database().await else {
        return;
    };
    let base = volundd::api::router(db.clone());
    let _owner = support::authenticated_router(&db, base).await;
    let actor = session::login(
        &db,
        LoginInput {
            email: "catalog-owner@example.test".into(),
            password: "catalog test password with enough entropy".into(),
            client_address: None,
            user_agent: None,
        },
        SessionPolicy::default(),
    )
    .await
    .unwrap();
    let root = std::env::temp_dir().join(format!("volund-service-log-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_volundd"));
    daemon
        .arg("serve")
        .env(
            "VOLUND_DATABASE_URL",
            std::env::var("VOLUND_TEST_DATABASE_URL").unwrap(),
        )
        .env("VOLUND_LISTEN_ADDR", &address)
        .env("VOLUND_SECURE_COOKIES", "false")
        .env_remove("VOLUND_BOOTSTRAP_TOKEN_FILE")
        .env("VOLUND_WEB_ROOT", root.join("web"))
        .env("VOLUND_DERIVED_ROOT", root.join("derived"))
        .env("VOLUND_SUPPORT_ROOT", root.join("support"));
    let mut child = daemon
        .stdout(Stdio::from(
            fs::File::create(root.join("stdout.log")).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(root.join("stderr.log")).unwrap(),
        ))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "isolated daemon exited before readiness"
            );
            if tokio::net::TcpStream::connect(&address).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let secrets = exercise_credentials_and_database_failure(&db, &address, &actor).await;
    let bundle = http(
        &address,
        "POST",
        "/api/v1/operations/support-bundle",
        json!({}),
        Some(&actor),
        "audit-bundle",
    )
    .await;
    assert_eq!(bundle.status, 200);
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    verify_diagnostic_surfaces(&db, &root, &actor, &secrets, &bundle.body).await;
    fs::remove_dir_all(root).unwrap();
}

async fn exercise_credentials_and_database_failure(
    db: &sqlx::PgPool,
    address: &str,
    actor: &LoginSession,
) -> Vec<String> {
    let password = "audit-personal-password-marker-0906";
    let replacement = "audit-reset-password-marker-0906";
    let invitation = http(
        address,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"live-log@example.test","displayName":"Live log","role":"viewer"}),
        Some(actor),
        "audit-invite",
    )
    .await;
    assert_eq!(invitation.status, 200);
    let invitation: Value = serde_json::from_slice(&invitation.body).unwrap();
    let token = invitation["activationToken"].as_str().unwrap();
    let accepted = http(
        address,
        "POST",
        "/api/v1/invitations/accept",
        json!({"token":token,"password":password}),
        None,
        "audit-accept",
    )
    .await;
    assert_eq!(accepted.status, 204);
    let reset = http(
        address,
        "POST",
        &format!(
            "/api/v1/users/{}/password",
            invitation["user"]["id"].as_str().unwrap()
        ),
        json!({"password":replacement}),
        Some(actor),
        "audit-reset",
    )
    .await;
    assert_eq!(reset.status, 204);
    assert!(reset.body.is_empty());
    sqlx::query("CREATE FUNCTION volund.audit_log_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'password=AUDIT_PRIVATE_PAYLOAD /srv/private/audit.step'; END $$")
        .execute(db).await.unwrap();
    sqlx::query("CREATE TRIGGER audit_log_fault BEFORE INSERT ON volund.user_invitations FOR EACH ROW EXECUTE FUNCTION volund.audit_log_fault()")
        .execute(db).await.unwrap();
    let failed = http(
        address,
        "POST",
        "/api/v1/users/invitations",
        json!({"email":"failure@example.test","displayName":"Failure","role":"viewer"}),
        Some(actor),
        "audit-invitation-failure",
    )
    .await;
    sqlx::query("DROP TRIGGER audit_log_fault ON volund.user_invitations")
        .execute(db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION volund.audit_log_fault()")
        .execute(db)
        .await
        .unwrap();
    assert_eq!(failed.status, 500);
    assert!(
        failed
            .headers
            .to_ascii_lowercase()
            .contains("x-request-id: audit-invitation-failure")
    );
    assert!(!String::from_utf8_lossy(&failed.body).contains("AUDIT_PRIVATE_PAYLOAD"));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM volund.users WHERE email='failure@example.test'")
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!(count, 0, "failed invitation must roll back its account");
    vec![password.into(), replacement.into(), token.into()]
}

#[tokio::test]
async fn real_daemon_startup_failure_has_safe_actionable_stderr() {
    let output = Command::new(env!("CARGO_BIN_EXE_volundd"))
        .arg("serve")
        .env("VOLUND_LISTEN_ADDR", "127.0.0.1:0")
        .env(
            "VOLUND_DATABASE_URL",
            "postgresql://audit:AUDIT_STARTUP_SECRET@127.0.0.1:not-a-port/audit_test",
        )
        .env("VOLUND_SECURE_COOKIES", "false")
        .env_remove("VOLUND_BOOTSTRAP_TOKEN_FILE")
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("AUDIT_STARTUP_SECRET"));
    let event: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(event["event"], "daemon.start_failed");
    assert_eq!(event["code"], "database_connect_failed");
    assert_eq!(event["severity"], "error");
}

async fn verify_diagnostic_surfaces(
    db: &sqlx::PgPool,
    root: &std::path::Path,
    actor: &LoginSession,
    secrets: &[String],
    bundle: &[u8],
) {
    let stdout = fs::read_to_string(root.join("stdout.log")).unwrap();
    let stderr = fs::read_to_string(root.join("stderr.log")).unwrap();
    assert!(stdout.contains("daemon_ready"));
    let event: Value = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["requestId"] == "audit-invitation-failure")
        .expect("correlated real stderr event");
    assert_eq!(event["severity"], "error");
    assert_eq!(event["event"], "http.request.completed");
    assert!(event["message"].as_str().unwrap().contains("status=500"));
    assert!(
        event["message"]
            .as_str()
            .unwrap()
            .contains("/users/invitations")
    );
    let stored: String = sqlx::query_scalar(
        "SELECT coalesce(json_agg(row_to_json(e))::text,'[]') FROM volund.operational_log_events e",
    )
    .fetch_one(db)
    .await
    .unwrap();
    let audit: String = sqlx::query_scalar(
        "SELECT coalesce(json_agg(row_to_json(e))::text,'[]') FROM volund.security_audit_events e",
    )
    .fetch_one(db)
    .await
    .unwrap();
    for surface in [
        stdout.as_bytes(),
        stderr.as_bytes(),
        stored.as_bytes(),
        audit.as_bytes(),
        bundle,
    ] {
        for secret in secrets.iter().map(String::as_str).chain([
            actor.session_token.as_str(),
            actor.csrf_token.as_str(),
            "AUDIT_PRIVATE_PAYLOAD",
        ]) {
            assert!(
                !surface
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes()),
                "secret marker found in diagnostic surface"
            );
        }
    }
}
