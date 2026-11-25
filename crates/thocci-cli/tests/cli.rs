use std::process::Command;

fn thocci_cmd(state_dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_thocci"));
    cmd.env("THOCCI_STATE_DIR", state_dir);
    cmd
}

#[test]
fn cli_offline_commands_work_end_to_end() {
    let state = std::env::temp_dir().join(format!("thocci-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).unwrap();

    // Stopped status exits with code 1 (matching Mechy).
    let out = thocci_cmd(&state).arg("status").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stopped (pack cherry-mx)"), "got: {stdout}");

    // Switch pack while stopped persists for next start.
    let out = thocci_cmd(&state).arg("banana-split").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack banana-split"), "got: {stdout}");

    // Status reflects saved pack.
    let out = thocci_cmd(&state).arg("status").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("stopped (pack banana-split)"),
        "got: {stdout}"
    );

    // Volume clamps and saves while stopped.
    let out = thocci_cmd(&state).args(["volume", "65"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("volume 65"), "got: {stdout}");

    // Packs listing marks active pack.
    let out = thocci_cmd(&state).arg("packs").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("* banana-split"), "got: {stdout}");
    assert!(stdout.contains("cherry-mx"), "got: {stdout}");

    // Direct pack switching by slug and by display name (multi-word).
    let out = thocci_cmd(&state).arg("creamy").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack creamy"), "got: {stdout}");

    let out = thocci_cmd(&state)
        .args(["Wooden", "Keyboard"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack wooden-keyboard"), "got: {stdout}");

    // Unknown pack exits non-zero.
    let out = thocci_cmd(&state).arg("not-a-real-pack").output().unwrap();
    assert!(!out.status.success());

    // Off while already stopped exits 0.
    let out = thocci_cmd(&state).arg("off").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("already stopped"), "got: {stdout}");

    // Guide and --help render the comprehensive terminal guide.
    for arg in ["--help", "guide"] {
        let out = thocci_cmd(&state).arg(arg).output().unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("ENGINE CONTROLS"), "arg={arg}: {stdout}");
        assert!(
            stdout.contains("VOLUME & SOUNDPACKS"),
            "arg={arg}: {stdout}"
        );
        assert!(!stdout.contains("\x1b["), "piped output should omit ANSI");
    }

    // Welcome banner exits 0.
    let out = thocci_cmd(&state).arg("--welcome").output().unwrap();
    assert!(out.status.success());

    let _ = std::fs::remove_dir_all(&state);
}
