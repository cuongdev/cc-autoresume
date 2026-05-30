use assert_cmd::Command;

#[test]
fn list_prints_with_empty_home() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cc-autoresume").unwrap()
        .env("HOME", home.path())
        .arg("list")
        .assert().success().stdout(predicates::str::contains("no pending resumes"));
}

#[test]
fn mode_sets_config() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cc-autoresume").unwrap()
        .env("HOME", home.path()).args(["mode","off"])
        .assert().success();
    let cfg = std::fs::read_to_string(home.path().join(".claude/auto-resume/config.json")).unwrap();
    assert!(cfg.contains("\"mode\": \"off\""));
}

#[test]
fn bad_mode_nonzero() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cc-autoresume").unwrap()
        .env("HOME", home.path()).args(["mode","bogus"])
        .assert().failure();
}

#[test]
fn token_prints_and_persists() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::cargo_bin("cc-autoresume").unwrap()
        .env("HOME", home.path()).arg("token")
        .assert().success();
    let token = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert_eq!(token.trim().len(), 33);
}
