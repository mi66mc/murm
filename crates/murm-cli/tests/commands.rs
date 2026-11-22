use murm_cli::*;
use murm_protocol::derive_public_key;
use std::process::{Command, Stdio};
fn command(dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_murm-cli"));
    c.arg("--data-dir").arg(dir).stdin(Stdio::null());
    c
}
#[tokio::test]
async fn profile_commands_match_service() {
    let d = tempfile::tempdir().unwrap();
    let public = derive_public_key(&[1; 32]).to_string();
    let output = command(d.path())
        .args(["profile", "import", "Ana", "--public-key", &public])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: ProfileSummary = serde_json::from_slice(&output.stdout).unwrap();
    let app = ClientService::open(d.path()).unwrap();
    assert_eq!(
        app.profile(value.id).await.unwrap().public_key.to_string(),
        public
    );
}
#[test]
fn public_import_never_unlocks() {
    let d = tempfile::tempdir().unwrap();
    let public = derive_public_key(&[1; 32]).to_string();
    let output = command(d.path())
        .args(["profile", "import", "Ana", "--public-key", &public])
        .output()
        .unwrap();
    assert!(output.status.success());
    let db = rusqlite::Connection::open(d.path().join("client.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM vault", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn secret_input_not_in_argv() {
    let d = tempfile::tempdir().unwrap();
    let output = command(d.path())
        .args(["profile", "import", "Ana", "--private-key", "not-supported"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("not-supported"));
}
#[tokio::test]
async fn cli_uses_same_database_as_tui() {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    let profile = app
        .create_profile(
            "From service",
            ProfileInput::ImportPublic(derive_public_key(&[1; 32])),
        )
        .await
        .unwrap();
    drop(app);
    let output = command(d.path())
        .args(["profile", "list"])
        .output()
        .unwrap();
    let profiles: Vec<ProfileSummary> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(profiles[0].id, profile.id);
}
#[test]
fn headless_never_hangs() {
    let d = tempfile::tempdir().unwrap();
    let mut child = command(d.path())
        .args(["profile", "create", "Ana"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("headless client waited for a secret");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
