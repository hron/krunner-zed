use std::path::Path;

use dirs::data_local_dir;
use rusqlite::{Connection, OpenFlags};
use zbus::{connection, interface};

struct ZedRunner;

fn get_projects() -> Vec<(String, String)> {
    let db_path = data_local_dir()
        .expect("no local data dir")
        .join("zed/db/0-stable/db.sqlite");

    let Ok(conn) = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };

    let Ok(mut stmt) = conn.prepare(
        "SELECT paths FROM workspaces WHERE paths IS NOT NULL AND paths != '' ORDER BY timestamp DESC",
    ) else {
        return Vec::new();
    };

    let rows = stmt.query_map([], |row| row.get::<_, String>(0));
    let Ok(rows) = rows else {
        return Vec::new();
    };

    rows.flatten()
        .filter(|p| !p.is_empty())
        .map(|path| {
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            (path, name)
        })
        .collect()
}

fn match_score(query: &str, name: &str, path: &str) -> Option<f64> {
    if query.is_empty() {
        return Some(0.5);
    }
    let q = query.to_lowercase();
    let n = name.to_lowercase();
    let p = path.to_lowercase();

    if n == q {
        Some(1.0)
    } else if n.contains(&q) {
        Some(0.9)
    } else if p.contains(&q) {
        Some(0.7)
    } else {
        None
    }
}

// Match tuple: (id, text, icon, type, relevance, properties{})
// type 3 = Plasma::QueryMatch::ExactMatch
type RemoteMatch = (String, String, String, i32, f64, std::collections::HashMap<String, zbus::zvariant::Value<'static>>);

#[interface(name = "org.kde.krunner1")]
impl ZedRunner {
    async fn actions(&self) -> Vec<(String, String, String)> {
        vec![]
    }

    #[zbus(name = "Match")]
    async fn match_query(&self, query: &str) -> Vec<RemoteMatch> {
        get_projects()
            .into_iter()
            .filter_map(|(path, name)| {
                match_score(query, &name, &path).map(|score| {
                    let mut props = std::collections::HashMap::new();
                    props.insert(
                        "subtext".to_string(),
                        zbus::zvariant::Value::new(path.clone()),
                    );
                    props.insert(
                        "category".to_string(),
                        zbus::zvariant::Value::new("Zed Projects".to_string()),
                    );
                    (path, name, "zed".to_string(), 3i32, score, props)
                })
            })
            .collect()
    }

    async fn run(&self, match_id: &str, _action_id: &str) {
        let _ = std::process::Command::new("zed")
            .arg(match_id)
            .spawn();
    }

    async fn teardown(&self) {}
}

#[tokio::main]
async fn main() -> zbus::Result<()> {
    let _conn = connection::Builder::session()?
        .name("dev.algus.krunner_zed")?
        .serve_at("/krunner_zed", ZedRunner)?
        .build()
        .await?;

    // Keep running until killed
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
