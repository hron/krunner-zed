use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use dirs::data_local_dir;
use freedesktop_desktop_entry::DesktopEntry;
use rusqlite::{Connection, OpenFlags};
use zbus::{interface, zvariant::Value};

#[derive(Debug, Clone)]
pub struct ZedInstance {
    pub label: String,
    pub exec: String,
    pub app_id: String,
    pub icon: String,
    pub db_path: PathBuf,
}

pub const SCHEMA_HANDLER_ID: &str = "x-scheme-handler/zed";

pub fn db_path_for_exec(exec: &str) -> PathBuf {
    let variant = if exec.to_lowercase().contains("dev") {
        "0-dev"
    } else {
        "0-stable"
    };
    data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join(format!("zed/db/{}/db.sqlite", variant))
}

static ZED_INSTANCES_CACHE: Mutex<Option<Vec<ZedInstance>>> = Mutex::const_new(None);

pub async fn find_zed_instances() -> Vec<ZedInstance> {
    if let Some(cached) = ZED_INSTANCES_CACHE.lock().await.as_ref() {
        return cached.clone();
    }
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
                .map(|types| types.contains(&SCHEMA_HANDLER_ID))
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

            // Desktop file ID: stem of the filename (e.g. "dev.zed.Zed" from "dev.zed.Zed.desktop")
            let app_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("zed")
                .to_string();

            let db_path = db_path_for_exec(&exec);

            if db_path.exists() {
                instances.push(ZedInstance {
                    label,
                    exec,
                    app_id,
                    icon,
                    db_path,
                });
            }
        }
    }

    // Deduplicate by db_path: prefer earlier XDG entries (user-local over system)
    let mut seen_dbs = std::collections::HashSet::new();
    instances.retain(|i| seen_dbs.insert(i.db_path.clone()));

    ZED_INSTANCES_CACHE.lock().await.replace(instances.clone());
    instances
}

pub fn query_projects(instance: &ZedInstance) -> Vec<(String, String)> {
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

    let mut seen = std::collections::HashSet::new();
    rows.flatten()
        .filter(|p| !p.is_empty())
        .filter_map(|raw| {
            let first = raw.split('\n').find(|s| !s.is_empty())?.to_string();
            if !seen.insert(first.clone()) {
                return None;
            }
            let name = Path::new(&first)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(first);
            Some((raw, name))
        })
        .collect()
}

pub fn match_score(query: &str, name: &str, path: &str) -> Option<f64> {
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
pub type RemoteMatch = (
    String,
    String,
    String,
    i32,
    f64,
    HashMap<String, Value<'static>>,
);

pub struct ZedRunner;

#[interface(name = "org.kde.krunner1")]
impl ZedRunner {
    pub async fn actions(&self) -> Vec<(String, String, String)> {
        vec![]
    }

    #[zbus(name = "Match")]
    pub async fn match_query(&self, query: &str) -> Vec<RemoteMatch> {
        let mut results = Vec::new();

        for instance in find_zed_instances().await {
            for (path, name) in query_projects(&instance) {
                let Some(score) = match_score(query, &name, &path) else {
                    continue;
                };

                // Encode exec + app_id + path so Run knows which binary and desktop file to use
                let match_id = format!("{}|{}|{}", instance.exec, instance.app_id, path);

                let mut props = HashMap::new();
                let display = if let Some(home) = dirs::home_dir() {
                    path.clone().replace(home.to_str().unwrap(), "~")
                } else {
                    path.clone()
                };
                props.insert("subtext".to_string(), Value::new(display));
                props.insert("category".to_string(), Value::new(instance.label.clone()));

                results.push((match_id, name, instance.icon.clone(), 3i32, score, props));
            }
        }

        results
    }

    pub async fn run(&self, match_id: &str, _action_id: &str) {
        // match_id = "<exec_path>|<app_id>|<project_path>" where project_path may be newline-separated
        let mut parts = match_id.splitn(3, '|');
        if let (Some(exec), Some(app_id), Some(project_path)) =
            (parts.next(), parts.next(), parts.next())
        {
            // Use the first path only; kstart --application accepts a single --url
            if let Some(first_path) = project_path.split('\n').find(|s| !s.is_empty()) {
                let url = format!("file://{}", first_path);

                // Try to spawn kstart first. If kstart is not found, fall back to
                // launching the exec binary directly.
                match std::process::Command::new("kstart")
                    .args(["--application", app_id, "--url", &url])
                    .spawn()
                {
                    Ok(_) => {}
                    Err(_) => {
                        let _ = std::process::Command::new(exec).arg(first_path).spawn();
                    }
                }
            }
        }
    }

    pub async fn teardown(&self) {}
}
