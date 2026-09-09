//! Exercise the real CLI with a fake camera, proving local captures never contact an upload server.
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn local_capture_and_no_upload_stay_out_of_upload_queue() {
    let root = std::env::temp_dir().join(format!("daily-mirror-local-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let camera = root.join("camera.sh");
    fs::write(&camera,"#!/bin/sh\nwhile [ $# -gt 0 ]; do\n if [ \"$1\" = --output ]; then shift; printf '\\377\\330test\\377\\331' > \"$1\"; exit 0; fi\n shift\ndone\nexit 1\n").unwrap();
    fs::set_permissions(&camera, fs::Permissions::from_mode(0o700)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let run = |mode: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_daily-mirror-device"))
            .current_dir(&root)
            .env("DAILY_MIRROR_CAPTURE_MODE", mode)
            .env("DAILY_MIRROR_CAMERA_PROFILE", "ov5647")
            .env("DAILY_MIRROR_CAMERA_COMMAND", &camera)
            .env("DAILY_MIRROR_CAMERA_ARGS", "")
            .env("DAILY_MIRROR_QUEUE_DIR", root.join("pending"))
            .env("DAILY_MIRROR_LOCAL_DIR", root.join("local"))
            .env(
                "DAILY_MIRROR_CAMERA_SETTINGS_PATH",
                root.join("orientation.json"),
            )
            .env(
                "DAILY_MIRROR_SERVER_URL",
                format!("http://{}", listener.local_addr().unwrap()),
            )
            .env("DAILY_MIRROR_UPLOAD_TOKEN", "must-not-be-used")
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run("local", &["capture-once"]).status.success());
    assert!(
        run("upload", &["capture-once", "--no-upload"])
            .status
            .success()
    );
    assert_eq!(fs::read_dir(root.join("local")).unwrap().count(), 2);
    assert_eq!(fs::read_dir(root.join("pending")).unwrap().count(), 0);
    assert!(run("local", &["retry"]).status.success());
    assert!(
        !run("local", &["upload", camera.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        listener.accept().is_err(),
        "local mode attempted an HTTP connection"
    );
    assert_eq!(fs::read_dir(root.join("local")).unwrap().count(), 2);
    fs::remove_dir_all(root).unwrap();
}
