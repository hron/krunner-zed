//! Integration tests for krunner-zed multi-path workspace matching.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use krunner_zed::ZedRunner;
use rusqlite::Connection;
use tempfile::TempDir;
use zbus::{connection, proxy};

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

struct TestContext {
    _tmp: TempDir,
    _dbus: DbusGuard,
    _server_conn: zbus::Connection,
}

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
}

/// Sets up a fake filesystem and a workspace containing multiple paths (directories and files).
async fn setup_test() -> (KRunnerProxy<'static>, TestContext) {
    let tmp = TempDir::new().unwrap();
    let share = tmp.path().join("home/.local/share");
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

    // Create paths inside the temp dir:
    // 1. dir1 (directory)
    // 2. dir2 (directory)
    // 3. file3 (file)
    // 4. nonexistent (does not exist, should be skipped)
    let dir1 = tmp.path().join("dir1");
    let dir2 = tmp.path().join("dir2");
    let file3 = tmp.path().join("file3");
    let nonexistent = tmp.path().join("nonexistent");

    std::fs::create_dir_all(&dir1).unwrap();
    std::fs::create_dir_all(&dir2).unwrap();
    std::fs::write(&file3, "hello").unwrap();

    let paths_value = format!(
        "{}\n{}\n{}\n{}",
        dir1.to_str().unwrap(),
        dir2.to_str().unwrap(),
        file3.to_str().unwrap(),
        nonexistent.to_str().unwrap()
    );

    conn.execute(
        "INSERT INTO workspaces (paths, timestamp) VALUES (?1, ?2)",
        rusqlite::params![&paths_value, 1i64],
    )
    .unwrap();

    let dbus = DbusGuard::start();

    let home = tmp.path().join("home");
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

    tokio::time::sleep(Duration::from_millis(50)).await;

    let client_conn = connection::Builder::address(dbus.address.as_str())
        .unwrap()
        .build()
        .await
        .expect("failed to connect client to test bus");

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

#[tokio::test]
async fn match_multi_path_workspace() {
    let (proxy, _ctx) = setup_test().await;

    // Empty query should return the workspace
    let results = proxy.match_query("").await.expect("Match call failed");
    assert_eq!(results.len(), 1, "expected exactly 1 workspace match");

    let (match_id, name, icon, _, _, props) = &results[0];

    // Name should be joined file names: "dir1, dir2, file3" (excluding nonexistent)
    assert_eq!(name, "dir1, dir2, file3");
    assert_eq!(icon, "zed");

    // Check that nonexistent was excluded, and others are joined in match_id and subtext
    assert!(match_id.contains("dir1"));
    assert!(match_id.contains("dir2"));
    assert!(match_id.contains("file3"));
    assert!(!match_id.contains("nonexistent"));

    let subtext = props.get("subtext").expect("subtext should be present");
    let subtext_str = subtext.downcast_ref::<String>().unwrap();

    // Subtext should format them beautifully separated by ", "
    assert!(subtext_str.contains("dir1"));
    assert!(subtext_str.contains("dir2"));
    assert!(subtext_str.contains("file3"));
    assert!(!subtext_str.contains("\n")); // Newlines should be replaced with ", "
    assert!(subtext_str.contains(", "));

    // Query for 'file3' (the file) should match the workspace
    let match_file = proxy.match_query("file3").await.unwrap();
    assert_eq!(match_file.len(), 1, "should match on file name query");
    assert_eq!(match_file[0].1, "dir1, dir2, file3");

    // Query for 'dir2' should match the workspace
    let match_dir = proxy.match_query("dir2").await.unwrap();
    assert_eq!(match_dir.len(), 1, "should match on dir name query");
    assert_eq!(match_dir[0].1, "dir1, dir2, file3");
}
