mod common;
use murm_cli::*;
use murm_protocol::*;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn relay_binary() -> PathBuf {
    std::env::var_os("MURM_RELAY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_BIN_EXE_murm-cli"))
                .parent()
                .unwrap()
                .join(format!("murm-relay{}", std::env::consts::EXE_SUFFIX))
        })
}
#[derive(Serialize)]
struct Config {
    database_path: PathBuf,
    listen: String,
    peers: Vec<String>,
    info: RelayInfo,
    sync_interval_seconds: u64,
}
struct Relay {
    child: Child,
    url: String,
}
impl Relay {
    fn start(dir: &Path, peers: Vec<String>, info: RelayInfo, listen: &str) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        let binary = relay_binary();
        assert!(
            binary.exists(),
            "Build both binaries first with cargo build --workspace, or set MURM_RELAY_BIN"
        );
        let config = Config {
            database_path: dir.join("relay.sqlite"),
            listen: listen.into(),
            peers,
            info,
            sync_interval_seconds: 1,
        };
        let path = dir.join("relay.toml");
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let mut child = Command::new(binary)
            .arg("--config")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reported = false;
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                if let Some(url) = line.strip_prefix("Murm relay listening on ")
                    && !reported
                {
                    let _ = send.send(url.to_owned());
                    reported = true;
                }
            }
        });
        let mut relay = Self {
            child,
            url: String::new(),
        };
        relay.url = recv
            .recv_timeout(Duration::from_secs(15))
            .expect("relay did not start");
        relay
    }
    fn address(&self) -> String {
        self.url.trim_start_matches("http://").into()
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn info() -> RelayInfo {
    let mut info = RelayInfo::default();
    info.rate_limits.clear();
    info
}
struct Flow {
    root: tempfile::TempDir,
    writer: PathBuf,
    reader: PathBuf,
    password: PathBuf,
    author: ProfileSummary,
}
impl Flow {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let writer = root.path().join("writer");
        let reader = root.path().join("reader");
        let password = root.path().join("password.txt");
        std::fs::write(&password, "uma senha longa apenas de teste").unwrap();
        let author: ProfileSummary = serde_json::from_value(cli(
            &writer,
            Some(&password),
            &["profile", "create", "Writer"],
        ))
        .unwrap();
        let public = derive_public_key(&[2; 32]).to_string();
        cli(
            &reader,
            None,
            &["profile", "import", "Reader", "--public-key", &public],
        );
        Self {
            root,
            writer,
            reader,
            password,
            author,
        }
    }
    fn publish(&self, url: &str, text: &str) -> (Draft, EventId) {
        cli(&self.writer, None, &["relay", "add", url]);
        let source = self.root.path().join("article.md");
        std::fs::write(&source, text).unwrap();
        let draft: Draft = serde_json::from_value(cli(
            &self.writer,
            None,
            &[
                "article",
                "draft",
                "--title",
                "Test article",
                "--source-file",
                source.to_str().unwrap(),
            ],
        ))
        .unwrap();
        let output = cli(
            &self.writer,
            Some(&self.password),
            &["article", "publish", &draft.id.to_string()],
        );
        assert_eq!(output["outbox"][0]["destinations"][0]["state"], "stored");
        (draft, output["id"].as_str().unwrap().parse().unwrap())
    }
}
fn cli(dir: &Path, password: Option<&Path>, args: &[&str]) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_murm-cli"));
    command.arg("--data-dir").arg(dir).stdin(Stdio::null());
    if let Some(path) = password {
        command.arg("--password-file").arg(path);
    }
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap()
}
async fn wait_event(url: &str, id: EventId) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let client = http();
    loop {
        if client
            .get(format!("{url}/events/{id}"))
            .send()
            .await
            .is_ok_and(|r| r.status() == 200)
        {
            return;
        }
        assert!(Instant::now() < deadline, "event never reached replica");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}
#[tokio::test]
async fn create_profile_publish_read() {
    let dir = tempfile::tempdir().unwrap();
    let relay = Relay::start(dir.path(), vec![], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (_, id) = f.publish(
        &relay.url,
        "# Complete source\n\nAn article with a footnote.[^n]\n\n[^n]: A source.",
    );
    cli(&f.reader, None, &["relay", "add", &relay.url]);
    let result = cli(&f.reader, None, &["fetch", &id.to_string()]);
    let e = parse_and_verify(
        &serde_json::to_vec(&result).unwrap(),
        ParseLimits::default(),
    )
    .unwrap();
    assert_eq!(e.event().signer, f.author.public_key);
    assert!(e.event().content.as_str().unwrap().contains("[^n]"));
}
#[tokio::test]
async fn two_relays_two_clients() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let rb = Relay::start(b.path(), vec![], info(), "127.0.0.1:0");
    let ra = Relay::start(a.path(), vec![rb.url.clone()], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (_, id) = f.publish(&ra.url, "two relays");
    wait_event(&rb.url, id).await;
    cli(&f.reader, None, &["relay", "add", &rb.url]);
    let result = cli(&f.reader, None, &["sync"]);
    assert_eq!(result[0]["sync"]["accepted"], 1);
    let app = ClientService::open(&f.reader).unwrap();
    assert_eq!(
        app.cached_event(id).await.unwrap().unwrap().event().signer,
        f.author.public_key
    );
}
#[tokio::test]
async fn stop_origin_read_replica() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let replica = Relay::start(b.path(), vec![], info(), "127.0.0.1:0");
    let origin = Relay::start(a.path(), vec![replica.url.clone()], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (_, id) = f.publish(&origin.url, "survives origin shutdown");
    wait_event(&replica.url, id).await;
    drop(origin);
    cli(&f.reader, None, &["relay", "add", &replica.url]);
    let event = cli(&f.reader, None, &["fetch", &id.to_string()]);
    assert_eq!(event["content"], "survives origin shutdown");
}
#[tokio::test]
async fn restart_retains_profiles_and_events() {
    let d = tempfile::tempdir().unwrap();
    let relay = Relay::start(d.path(), vec![], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (_, id) = f.publish(&relay.url, "durable");
    drop(relay);
    let relay = Relay::start(d.path(), vec![], info(), "127.0.0.1:0");
    assert_eq!(
        http()
            .get(format!("{}/events/{id}", relay.url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let profiles = cli(&f.writer, None, &["profile", "list"]);
    assert_eq!(profiles[0]["id"], f.author.id.to_string());
    assert_eq!(profiles[0]["locked"], true);
}
#[tokio::test]
async fn offline_edit_then_same_event_retry() {
    let d = tempfile::tempdir().unwrap();
    let relay = Relay::start(d.path(), vec![], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (draft, _) = f.publish(&relay.url, "first source");
    let address = relay.address();
    drop(relay);
    let app = ClientService::open(&f.writer).unwrap();
    app.unlock(
        SecretString::new("uma senha longa apenas de teste".into()),
        app.begin_vault_action(),
    )
    .await
    .unwrap();
    let mut edit = app
        .edit_publication(f.author.id, &draft.publication)
        .await
        .unwrap();
    edit.content = json!("offline revision");
    let edit = app.save_draft(f.author.id, edit).await.unwrap();
    let id = app.publish(f.author.id, edit.id).await.unwrap();
    drop(app);
    let relay = Relay::start(d.path(), vec![], info(), &address);
    cli(
        &f.writer,
        None,
        &["outbox", "retry", "--event", &id.to_string()],
    );
    let body = http()
        .get(format!("{}/events/{id}", relay.url))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let e = parse_and_verify(&body, ParseLimits::default()).unwrap();
    assert_eq!(e.event().id, id);
    assert_eq!(e.event().content, "offline revision");
    assert_eq!(e.event().header["revision"], 2);
}
#[tokio::test]
async fn withdrawal_and_body_expiry() {
    let d = tempfile::tempdir().unwrap();
    let relay = Relay::start(d.path(), vec![], info(), "127.0.0.1:0");
    let f = Flow::new();
    let (draft, old) = f.publish(&relay.url, "retained older body");
    let address = relay.address();
    drop(relay);
    let mut short = info();
    short.kind_policies.overrides[0].retention_seconds = Some(Some(1));
    short.kind_policies.overrides[0].max_retention_seconds = Some(Some(1));
    let relay = Relay::start(d.path(), vec![], short, &address);
    let output = cli(
        &f.writer,
        Some(&f.password),
        &["article", "withdraw", &draft.publication],
    );
    let id = output["id"].as_str().unwrap();
    let client = http();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if client
            .get(format!("{}/events/{id}", relay.url))
            .send()
            .await
            .unwrap()
            .status()
            == 404
        {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        client
            .get(format!("{}/events/{old}", relay.url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let response = client
        .post(format!("{}/events/query", relay.url))
        .json(&json!({"filters":[{"kinds":[1]}],"limit":10,"view":"current"}))
        .send()
        .await
        .unwrap();
    assert!(
        response.json::<Value>().await.unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn stale_cursor_recovers() {
    let d = tempfile::tempdir().unwrap();
    let mut policy = info();
    policy.limits.sync_cursor_seconds = 1;
    let relay = Relay::start(d.path(), vec![], policy, "127.0.0.1:0");
    let f = Flow::new();
    f.publish(&relay.url, "one");
    cli(&f.reader, None, &["relay", "add", &relay.url]);
    cli(&f.reader, None, &["sync"]);
    let cursor: String = rusqlite::Connection::open(f.reader.join("client.sqlite"))
        .unwrap()
        .query_row("SELECT cursor FROM sync_cursors", [], |r| r.get(0))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let client = http();
    loop {
        let response = client
            .post(format!("{}/events/sync", relay.url))
            .json(&json!({"filters":[{"kinds":[1]}],"cursor":cursor,"limit":1}))
            .send()
            .await
            .unwrap();
        if response.status() == 410 {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (_, id) = f.publish(&relay.url, "two");
    cli(&f.reader, None, &["sync"]);
    let app = ClientService::open(&f.reader).unwrap();
    assert!(app.cached_event(id).await.unwrap().is_some());
}
#[tokio::test]
async fn storage_failure_does_not_ack() {
    let d = tempfile::tempdir().unwrap();
    let relay = Relay::start(d.path(), vec![], info(), "127.0.0.1:0");
    rusqlite::Connection::open(d.path().join("relay.sqlite")).unwrap().execute_batch("CREATE TRIGGER reject_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'test disk failure'); END;").unwrap();
    let e = common::event("fail", 1);
    let response = http()
        .post(format!("{}/events", relay.url))
        .json(&e)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.json::<Problem>().await.unwrap().code,
        ProblemCode::TemporarilyUnavailable
    );
    assert_eq!(
        http()
            .get(format!("{}/events/{}", relay.url, e.event().id))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
#[test]
fn interactive_starts_tui_and_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_murm-cli"));
    command.arg("--data-dir");
    command.arg(dir.path());
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, recv) = std::sync::mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if send.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut bytes = Vec::new();
    let mut answered_cursor_queries = 0;
    let shown = loop {
        if let Ok(chunk) = recv.recv_timeout(Duration::from_millis(50)) {
            bytes.extend_from_slice(&chunk);
        }
        // Windows ConPTY requests the inherited cursor before launching the child.
        let queries = bytes.windows(4).filter(|w| *w == b"\x1b[6n").count();
        while answered_cursor_queries < queries {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            answered_cursor_queries += 1;
        }
        let output = String::from_utf8_lossy(&bytes);
        if output.contains("Murm") && output.contains("Profiles") && output.contains("?1049h") {
            break true;
        }
        if child.try_wait().unwrap().is_some() || Instant::now() > deadline {
            break false;
        }
    };
    if !shown {
        let _ = child.kill();
        let _ = child.wait();
        panic!(
            "interactive TUI not rendered: {}",
            String::from_utf8_lossy(&bytes)
        );
    }
    // An 80 ms/event input loop would need at least eight seconds for this burst.
    writer.write_all("23451".repeat(20).as_bytes()).unwrap();
    writer.write_all(&[3]).unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("TUI did not exit on Ctrl-C");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
