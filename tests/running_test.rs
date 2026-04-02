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
async fn run_with_and_without_kstart() {
    let td = tempdir().unwrap();
    let td_path = td.path();

    // 1) Create the fallback exec only (no kstart)
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

    // PATH contains only td
    unsafe { std::env::set_var("PATH", td_path) };

    let match_id = "zed-fake|com.example.Zed|/tmp/some/project";

    let runner = ZedRunner;
    runner.run(match_id, "").await;

    let got_exec = wait_for_file(&exec_out).await;
    assert!(
        got_exec.contains("/tmp/some/project"),
        "fallback exec should receive the path, got: {:?}",
        got_exec
    );

    // 2) Now add kstart to the same PATH directory
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

    // PATH already points to td, so newly created kstart will be found
    runner.run(match_id, "").await;

    let got_kstart = wait_for_file(&kstart_out).await;
    assert!(
        got_kstart.contains("--application"),
        "kstart should be invoked with --application, got: {:?}",
        got_kstart
    );
    assert!(got_kstart.contains("com.example.Zed"));
    assert!(got_kstart.contains("file:///tmp/some/project"));
}
