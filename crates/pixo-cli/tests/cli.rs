use fs2::FileExt;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Harness {
    dir: PathBuf,
    config: PathBuf,
}
impl Harness {
    fn new() -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/cli-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&dir).unwrap();
        Self {
            config: dir.join("config.toml"),
            dir,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pixo"));
        command
            .arg("--config")
            .arg(&self.config)
            .arg("--json")
            .env_remove("PIXO_DEVICE")
            .env("PIXO_STATE_DIR", self.dir.join("state"));
        command
    }
    fn run(&self, args: &[&str]) -> (Output, Value) {
        let output = self.command().args(args).output().unwrap();
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "invalid JSON {e}: {} stderr {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output, value)
    }
    fn ok(&self, args: &[&str]) -> Value {
        let (output, value) = self.run(args);
        assert!(output.status.success(), "{value}");
        assert_eq!(value["ok"], true);
        value["result"].clone()
    }
    fn device(&self) {
        self.ok(&["device", "add", "desk", "--address", "00:11:22:33:44:55"]);
        self.ok(&["device", "use", "desk"]);
    }
    fn image(&self) -> PathBuf {
        let path = self.dir.join("wide.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        path
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn help_and_json_parse_errors_need_no_configuration() {
    let h = Harness::new();
    let output = h.command().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("scene"));
    assert!(help.contains("one invocation") || help.contains("invocation"));
    let (output, error) = h.run(&["stored", "0"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(error["error"]["category"], "invalid_input");
    assert!(!h.config.exists());
}

#[test]
fn scene_recipes_round_trip_without_touching_device() {
    let h = Harness::new();
    h.ok(&[
        "scene",
        "add",
        "build-running",
        "--tab",
        "2",
        "--brightness",
        "35",
    ]);
    let scene = h.ok(&["scene", "show", "build-running"]);
    assert_eq!(scene["scene"]["source"], "tab");
    assert_eq!(scene["scene"]["tab"], 2);
    let plan = h.ok(&["scene", "play", "build-running", "--dry-run"]);
    assert_eq!(plan["scene"], "build-running");
    assert_eq!(plan["plan"]["tab"], 2);
    assert_eq!(plan["brightness"], 35);
    let (output, _) = h.run(&["scene", "add", "build-running", "--tab", "1"]);
    assert_eq!(output.status.code(), Some(2));
    h.ok(&["scene", "add", "build-running", "--tab", "1", "--replace"]);
    h.ok(&["scene", "remove", "build-running"]);
    assert!(h.ok(&["scene", "list"])["scenes"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[test]
fn import_is_explicit_and_preserves_source_and_default_selection() {
    let h = Harness::new();
    let source = h.dir.join("go.json");
    let bytes =
        br#"{"transport":"rfcomm","mac":"00:11:22:33:44:55","channel":1,"listenAddr":":8377"}"#;
    fs::write(&source, bytes).unwrap();
    h.ok(&[
        "device",
        "import-go",
        source.to_str().unwrap(),
        "--name",
        "desk",
        "--profile",
        "pixoo-max",
        "--size",
        "32",
    ]);
    h.ok(&["device", "use", "desk"]);
    assert_eq!(h.ok(&["device", "list"])["default_device"], "desk");
    assert_eq!(fs::read(source).unwrap(), bytes);
    let doctor = h.ok(&["doctor"]);
    assert_eq!(doctor["connection_tested"], false);
}

#[test]
fn media_previews_use_prepared_pixels_and_never_overwrite() {
    let h = Harness::new();
    let source = h.image();
    let preview = h.dir.join("contain.png");
    let result = h.ok(&[
        "show",
        source.to_str().unwrap(),
        "--output",
        preview.to_str().unwrap(),
    ]);
    assert_eq!(result["device_connected"], false);
    let image = image::open(&preview).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (32, 32));
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(16, 16).0, [255, 0, 0, 255]);
    let before = fs::read(&preview).unwrap();
    let (output, _) = h.run(&["blank", "--output", preview.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read(preview).unwrap(), before);
    let dry = h.ok(&["save", source.to_str().unwrap(), "--tab", "1", "--dry-run"]);
    assert_eq!(dry["plan"]["media"]["source_width"], 16);
    assert_eq!(dry["plan"]["media"]["source_height"], 8);
    assert_eq!(dry["plan"]["expected_completion"], "custom_mode_status");
    let (output, error) = h.run(&["save", source.to_str().unwrap(), "--tab", "1"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--replace"));
}

#[test]
fn text_supports_stdin_wrapping_scrolling_and_glyph_errors() {
    let h = Harness::new();
    let png = h.dir.join("pass.png");
    h.ok(&[
        "text",
        "PASS",
        "--color",
        "green",
        "--output",
        png.to_str().unwrap(),
    ]);
    let image = image::open(png).unwrap().to_rgba8();
    assert!(image.pixels().any(|p| p.0 == [0, 255, 0, 255]));
    let gif = h.dir.join("scroll.gif");
    let result = h.ok(&[
        "text",
        "BUILD PASSED",
        "--overflow",
        "scroll",
        "--step",
        "80ms",
        "--output",
        gif.to_str().unwrap(),
    ]);
    assert_eq!(result["plan"]["media"]["frames"], 129);
    let animation = divoom_bluetooth::media::Animation::from_gif(
        fs::File::open(gif).unwrap(),
        divoom_bluetooth::media::Dimension::ThirtyTwo,
    )
    .unwrap();
    assert_eq!(animation.frames().len(), 129);
    assert!(animation.frames().iter().all(|f| f.duration_ms() == 80));
    h.ok(&["text", "HELLO", "--overflow", "wrap", "--dry-run"]);
    let (output, error) = h.run(&["text", "café", "--dry-run"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("U+00E9"));
    let mut child = h
        .command()
        .args(["text", "--stdin", "--dry-run"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"PASS\n").unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
}

#[test]
fn file_and_text_scenes_survive_fresh_processes_and_working_directories() {
    let h = Harness::new();
    let image = h.image();
    h.ok(&["scene", "add", "picture", "--file", image.to_str().unwrap()]);
    h.ok(&["scene", "add", "message", "--text", "OK", "--color", "cyan"]);
    let output = h
        .command()
        .args(["scene", "play", "picture", "--dry-run"])
        .current_dir(&h.dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let message = h.ok(&["scene", "play", "message", "--dry-run"]);
    assert_eq!(message["plan"]["media"]["frames"], 1);
    h.ok(&["scene", "upload", "message", "--tab", "3", "--dry-run"]);
}

#[test]
fn config_and_device_leases_reject_competing_processes() {
    let h = Harness::new();
    h.device();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(h.config.with_extension("toml.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let (output, error) = h.run(&["scene", "add", "busy", "--tab", "1"]);
    assert_eq!(output.status.code(), Some(3), "{error}");
    FileExt::unlock(&lock).unwrap();
    let lockdir = h.dir.join("state/locks");
    fs::create_dir_all(&lockdir).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lockdir.join("classic-001122334455.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let (output, error) = h.run(&["status"]);
    assert_eq!(output.status.code(), Some(3), "{error}");
    assert_eq!(error["device"], "desk");
    assert_eq!(error["error"]["delivery_uncertain"], false);
}

#[test]
fn legacy_controller_guard_stops_before_bluetooth_connection() {
    let h = Harness::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    h.ok(&[
        "device",
        "add",
        "desk",
        "--address",
        "00:11:22:33:44:55",
        "--controller",
        &endpoint,
    ]);
    h.ok(&["device", "use", "desk"]);
    let (output, error) = h.run(&["status"]);
    assert_eq!(output.status.code(), Some(3), "{error}");
    assert!(
        matches!(
            error["error"]["category"].as_str(),
            Some("busy" | "ownership_check")
        ),
        "{error}"
    );
    assert_eq!(error["error"]["delivery_uncertain"], false);
    h.ok(&["scene", "add", "idle", "--tab", "1"]);
    h.ok(&["scene", "play", "idle", "--dry-run"]);
}

#[test]
fn explicit_config_works_without_home_environment() {
    let h = Harness::new();
    let output = h
        .command()
        .args(["device", "list"])
        .env_remove("APPDATA")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn import_rejects_present_invalid_connection_fields_without_writing_config() {
    for (field, value) in [
        ("channel", serde_json::json!("2")),
        ("channel", serde_json::json!(-1)),
        ("channel", serde_json::json!(null)),
        ("listenAddr", serde_json::json!(123)),
        ("listenAddr", serde_json::json!(null)),
    ] {
        let h = Harness::new();
        let source = h.dir.join("invalid-go.json");
        let mut legacy = serde_json::json!({"transport":"rfcomm","mac":"00:11:22:33:44:55"});
        legacy[field] = value;
        let bytes = serde_json::to_vec(&legacy).unwrap();
        fs::write(&source, &bytes).unwrap();
        let (output, error) = h.run(&[
            "device",
            "import-go",
            source.to_str().unwrap(),
            "--name",
            "desk",
            "--profile",
            "pixoo-max",
            "--size",
            "32",
        ]);
        assert_eq!(output.status.code(), Some(2), "{field}: {error}");
        assert!(!h.config.exists());
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }
    let h = Harness::new();
    let source = h.dir.join("default-go.json");
    fs::write(
        &source,
        br#"{"transport":"rfcomm","mac":"00:11:22:33:44:55"}"#,
    )
    .unwrap();
    h.ok(&[
        "device",
        "import-go",
        source.to_str().unwrap(),
        "--name",
        "desk",
        "--profile",
        "pixoo-max",
        "--size",
        "32",
    ]);
    let devices = h.ok(&["device", "list"]);
    assert_eq!(devices["devices"]["desk"]["channel"], 1);
    assert_eq!(devices["devices"]["desk"]["controller"], ":8377");
}

#[test]
fn doctor_reports_validation_scope_as_selected_device() {
    let h = Harness::new();
    h.device();
    let mut contents = fs::read_to_string(&h.config).unwrap();
    contents.push_str("\n[devices.other]\naddress = '00:11:22:33:44:66'\nchannel = 31\nprofile = 'pixoo-max'\nsize = 32\n");
    fs::write(&h.config, contents).unwrap();
    let result = h.ok(&["doctor"]);
    assert_eq!(result["device_configuration"], "valid");
    assert!(result.get("configuration").is_none());
}
