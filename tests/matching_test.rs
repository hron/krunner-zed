//! Integration tests for krunner-zed.
//!
//! Each test spins up an isolated D-Bus session daemon and a temporary directory
//! that acts as the fake XDG home (containing a Zed `.desktop` file and a
//! pre-populated `db.sqlite`).
//!
//! `setup_test` returns a `KRunnerProxy<'static>` (the proxy owns its
//! connection via an internal `Arc`) alongside a `TestContext` that must be
//! kept alive for the duration of the test to keep the temporary filesystem,
//! the daemon process, and the temp directory alive.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use krunner_zed::ZedRunner;
use rusqlite::Connection;
use tempfile::TempDir;
use zbus::{connection, proxy};

// Note: tests in this file no longer serialize via an ENV_LOCK. The file
// contains a single integration test which performs multiple queries while
// holding its own environment configuration for the duration of the test.

// ---------------------------------------------------------------------------
// D-Bus daemon guard
// ---------------------------------------------------------------------------

/// Owns a `dbus-daemon` child process and kills it on drop.
struct DbusGuard {
    child: Child,
    pub address: String,
}

impl DbusGuard {
    fn start() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn dbus-daemon; is it installed?");

        let stdout = child.stdout.take().expect("no stdout from dbus-daemon");
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();

        reader
            .read_line(&mut address)
            .expect("dbus-daemon did not print an address");
        let address = address.trim().to_string();
        assert!(!address.is_empty(), "dbus-daemon printed an empty address");

        DbusGuard { child, address }
    }
}

impl Drop for DbusGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Fake filesystem helpers
// ---------------------------------------------------------------------------

/// Creates the directory tree expected by `find_zed_instances` and
/// `db_path_for_channel` inside `base`:
///
/// ```text
/// <base>/
///   home/
///     .local/share/
///       applications/
///         dev.zed.Zed.desktop
///       zed/db/0-stable/
///         db.sqlite
/// ```
fn setup_fake_fs(base: &TempDir, project_paths: &[&str]) -> PathBuf {
    let share = base.path().join("home/.local/share");
    let apps_dir = share.join("applications");
    let db_dir = share.join("zed/db/0-stable");

    std::fs::create_dir_all(&apps_dir).unwrap();
    std::fs::create_dir_all(&db_dir).unwrap();

    let desktop = "[Desktop Entry]\n\
         Type=Application\n\
         Name=Zed\n\
         Icon=zed\n\
         Exec=/usr/bin/zed %U\n\
         MimeType=x-scheme-handler/zed;\n\
         Categories=Development;\n";
    std::fs::write(apps_dir.join("dev.zed.Zed.desktop"), desktop).unwrap();

    let db_path = db_dir.join("db.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE workspaces (
            id        INTEGER PRIMARY KEY,
            paths     TEXT,
            timestamp INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();

    for (i, path) in project_paths.iter().enumerate() {
        let true_path = format!("{}{}", base.path().to_str().unwrap(), path);
        std::fs::create_dir_all(&true_path).unwrap();
        conn.execute(
            "INSERT INTO workspaces (paths, timestamp) VALUES (?1, ?2)",
            rusqlite::params![&true_path, i as i64],
        )
        .unwrap();
    }

    db_path
}

// ---------------------------------------------------------------------------
// D-Bus proxy for org.kde.krunner1
// ---------------------------------------------------------------------------

#[proxy(
    interface = "org.kde.krunner1",
    default_service = "dev.algus.krunner_zed",
    default_path = "/krunner_zed"
)]
trait KRunner {
    #[zbus(name = "Match")]
    #[allow(clippy::type_complexity)]
    async fn match_query(
        &self,
        query: &str,
    ) -> zbus::Result<
        Vec<(
            String,
            String,
            String,
            i32,
            f64,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        )>,
    >;

    async fn actions(&self) -> zbus::Result<Vec<(String, String, String)>>;
}

// ---------------------------------------------------------------------------
// TestContext + setup_test
// ---------------------------------------------------------------------------

/// Keeps test resources alive for the duration of a test.
///
/// Must be bound to a local (e.g. `let _ctx = ...`) so it is dropped only
/// after all assertions are done.  Dropping it kills the daemon and deletes
/// the temp directory.
struct TestContext {
    _tmp: TempDir,
    _dbus: DbusGuard,
    _server_conn: zbus::Connection,
}

/// Set up an isolated test environment and return a ready-to-use proxy.
///
/// The returned `TestContext` must stay alive for the duration of the test.
/// The `KRunnerProxy<'static>` owns its connection internally (via `Arc`) and
/// can be used directly without borrowing from `TestContext`.
async fn setup_test(project_paths: &[&str]) -> (KRunnerProxy<'static>, TestContext) {
    // Ensure a clean state for the test.

    let tmp = TempDir::new().unwrap();
    setup_fake_fs(&tmp, project_paths);

    let dbus = DbusGuard::start();

    let home = tmp.path().join("home");
    let share = home.join(".local/share");
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", &share);
        std::env::set_var("XDG_DATA_DIRS", "");
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &dbus.address);
    }

    let server_conn = connection::Builder::address(dbus.address.as_str())
        .unwrap()
        .name("dev.algus.krunner_zed")
        .unwrap()
        .serve_at("/krunner_zed", ZedRunner)
        .unwrap()
        .build()
        .await
        .expect("failed to register ZedRunner on test bus");

    // Give the daemon a moment to process the name registration.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Build the client connection and clone it into the proxy.  The proxy
    // holds its own Arc handle so it has no lifetime dependency on the local.
    let client_conn = connection::Builder::address(dbus.address.as_str())
        .unwrap()
        .build()
        .await
        .expect("failed to connect client to test bus");

    // KRunnerProxy::builder clones the connection internally, producing a
    // 'static proxy (all string parameters come from &'static str literals
    // in the #[proxy] attribute defaults).
    let proxy = KRunnerProxy::builder(&client_conn)
        .build()
        .await
        .expect("failed to create KRunnerProxy");

    let ctx = TestContext {
        _tmp: tmp,
        _dbus: dbus,
        _server_conn: server_conn,
    };

    (proxy, ctx)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Run a simple integration flow in a single test: first query with an empty
/// string to ensure all projects are returned, then run a specific query that
/// should match only one project.
#[tokio::test]
async fn match_all_then_specific_query() {
    let (proxy, _ctx) = setup_test(&[
        "/home/user/projects/alpha",
        "/home/user/projects/beta",
        "/home/user/projects/my-app",
    ])
    .await;

    // First: empty query should return all projects (alpha, beta, my-app)
    let results = proxy.match_query("").await.expect("Match call failed");
    let names: Vec<&str> = results.iter().map(|(_, name, ..)| name.as_str()).collect();
    assert!(
        names.contains(&"alpha"),
        "expected 'alpha' in results, got: {names:?}"
    );
    assert!(
        names.contains(&"beta"),
        "expected 'beta' in results, got: {names:?}"
    );
    assert!(
        names.contains(&"my-app"),
        "expected 'my-app' in results, got: {names:?}"
    );

    // Second: query for 'my-app' should return only that project with score 1.0
    let specific = proxy.match_query("my-app").await.unwrap();
    assert_eq!(specific.len(), 1, "only 'my-app' should match");
    let (_, name, _, _, score, _) = &specific[0];
    assert_eq!(name, "my-app");
    assert!(
        (*score - 1.0).abs() < f64::EPSILON,
        "expected score 1.0 for exact match, got {score}"
    );
}
