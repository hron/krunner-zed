use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dirs::data_local_dir;
use freedesktop_desktop_entry::DesktopEntry;
use rusqlite::{Connection, OpenFlags};
use zbus::{connection, interface, zvariant::Value};

struct ZedRunner;

#[derive(Debug)]
struct ZedInstance {
    label: String,
    exec: String,
    icon: String,
    db_path: PathBuf,
}

fn db_path_for_exec(exec: &str) -> PathBuf {
    let variant = if exec.to_lowercase().contains("dev") {
        "0-dev"
    } else {
        "0-stable"
    };
    data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join(format!("zed/db/{}/db.sqlite", variant))
}

fn find_zed_instances() -> Vec<ZedInstance> {
    let xdg = xdg::BaseDirectories::new();

    // Collect all application dirs: data_home + data_dirs
    let mut app_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = xdg.get_data_home() {
        app_dirs.push(home.join("applications"));
    }
    for dir in xdg.get_data_dirs() {
        app_dirs.push(dir.join("applications"));
    }

    let locales: Vec<String> = vec![];
    let mut instances = Vec::new();

    for dir in app_dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(de) = DesktopEntry::from_path(&path, Some(&locales)) else {
                continue;
            };

            // Only consider apps that handle the zed URI scheme
            let is_zed = de
                .mime_type()
                .map(|types| types.iter().any(|t| *t == "x-scheme-handler/zed"))
                .unwrap_or(false);

            if !is_zed {
                continue;
            }

            let Some(exec_field) = de.exec() else {
                continue;
            };

            // Take only the binary path (first token, strip any %U/%F args)
            let exec = exec_field
                .split_whitespace()
                .next()
                .unwrap_or(exec_field)
                .to_string();

            let label = de
                .name(&locales)
                .map(|n| n.into_owned())
                .unwrap_or_else(|| "Zed".to_string());

            let icon = de.icon().unwrap_or("zed").to_string();

            let db_path = db_path_for_exec(&exec);

            if db_path.exists() {
                instances.push(ZedInstance { label, exec, icon, db_path });
            }
        }
    }

    // Deduplicate by db_path: prefer earlier XDG entries (user-local over system)
    let mut seen_dbs = std::collections::HashSet::new();
    instances.retain(|i| seen_dbs.insert(i.db_path.clone()));
    instances
}

fn query_projects(instance: &ZedInstance) -> Vec<(String, String)> {
    let Ok(conn) = Connection::open_with_flags(
        &instance.db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };

    let Ok(mut stmt) = conn.prepare(
        "SELECT paths FROM workspaces WHERE paths IS NOT NULL AND paths != '' ORDER BY timestamp DESC",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
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

// Match tuple: (id, text, iconName, type, relevance, properties)
// type 3 = Plasma::QueryMatch::ExactMatch
type RemoteMatch = (String, String, String, i32, f64, HashMap<String, Value<'static>>);

#[interface(name = "org.kde.krunner1")]
impl ZedRunner {
    async fn actions(&self) -> Vec<(String, String, String)> {
        vec![]
    }

    #[zbus(name = "Match")]
    async fn match_query(&self, query: &str) -> Vec<RemoteMatch> {
        let instances = find_zed_instances();
        let mut results = Vec::new();

        for instance in &instances {
            for (path, name) in query_projects(instance) {
                let Some(score) = match_score(query, &name, &path) else {
                    continue;
                };

                // Encode exec + path so Run knows which binary to use
                let match_id = format!("{}|{}", instance.exec, path);

                let mut props = HashMap::new();
                props.insert("subtext".to_string(), Value::new(path.clone()));
                props.insert("category".to_string(), Value::new(instance.label.clone()));

                results.push((
                    match_id,
                    name,
                    instance.icon.clone(),
                    3i32,
                    score,
                    props,
                ));
            }
        }

        results
    }

    async fn run(&self, match_id: &str, _action_id: &str) {
        // match_id = "<exec_path>|<project_path>"
        if let Some((exec, project_path)) = match_id.split_once('|') {
            let _ = std::process::Command::new(exec).arg(project_path).spawn();
        }
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

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
