use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use tempfile::tempdir;

use krunner_zed::ZedRunner;

async fn wait_for_file(path: &std::path::Path) -> String {
    for _ in 0..50 {
        if path.exists() {
            return fs::read_to_string(path).unwrap_or_default();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // final attempt
    if path.exists() {
        fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    }
}

#[tokio::test]
async fn run_multi_path_workspace_filtering_files() {
    let td = tempdir().unwrap();
    let td_path = td.path();

    // Create the test paths:
    // 1. dir1 (directory)
    // 2. dir2 (directory)
    // 3. file3 (file)
    let dir1 = td_path.join("dir1");
    let dir2 = td_path.join("dir2");
    let file3 = td_path.join("file3");

    fs::create_dir_all(&dir1).unwrap();
    fs::create_dir_all(&dir2).unwrap();
    fs::write(&file3, "hello").unwrap();

    // 1) Test with direct fallback execution only (no kstart)
    let exec_out = td_path.join("exec.out");
    let exec_script = td_path.join("zed-fake");
    let script_content = format!(
        "#!/bin/sh\nprintf \"%s\\n\" \"$@\" > '{}'\n",
        exec_out.display()
    );
    fs::write(&exec_script, script_content).unwrap();
    let mut perms = fs::metadata(&exec_script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&exec_script, perms).unwrap();

    // Set PATH to contain only td_path
    unsafe { std::env::set_var("PATH", td_path) };

    // match_id = "<exec_path>|<app_id>|<project_paths>"
    let match_id = format!(
        "{}|com.example.Zed|{}\n{}\n{}",
        exec_script.display(),
        dir1.display(),
        dir2.display(),
        file3.display()
    );

    let runner = ZedRunner;
    runner.run(&match_id, "").await;

    let got_exec = wait_for_file(&exec_out).await;

    // Check that --new and only the directories dir1 and dir2 are passed (file3 is filtered out)
    assert!(got_exec.contains("--new"), "expected --new flag");
    assert!(got_exec.contains("dir1"), "expected dir1 in arguments");
    assert!(got_exec.contains("dir2"), "expected dir2 in arguments");
    assert!(
        !got_exec.contains("file3"),
        "expected file3 to be filtered out"
    );

    // 2) Test with kstart
    let kstart_out = td_path.join("kstart.out");
    let kstart_script = td_path.join("kstart");
    let kstart_content = format!(
        "#!/bin/sh\nprintf \"%s\\n\" \"$@\" > '{}'\n",
        kstart_out.display()
    );
    fs::write(&kstart_script, kstart_content).unwrap();
    let mut perms = fs::metadata(&kstart_script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&kstart_script, perms).unwrap();

    runner.run(&match_id, "").await;

    let got_kstart = wait_for_file(&kstart_out).await;

    // Check that kstart receives the arguments
    assert!(got_kstart.contains("--"), "expected -- flag");
    assert!(got_kstart.contains("--new"), "expected --new flag");
    assert!(got_kstart.contains("zed-fake"), "expected zed-fake binary");
    assert!(got_kstart.contains("dir1"), "expected dir1 in arguments");
    assert!(got_kstart.contains("dir2"), "expected dir2 in arguments");
    assert!(
        !got_kstart.contains("file3"),
        "expected file3 to be filtered out"
    );
}
