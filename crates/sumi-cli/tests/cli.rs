use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn sumi(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sumi"))
        .args(args)
        .output()
        .unwrap()
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(test);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn converts_a_file() {
    let dir = temp_dir("converts_a_file");
    let output = dir.join("out.pdf");
    let result = sumi(&[
        &fixture("libreoffice_table.pdf"),
        "-o",
        output.to_str().unwrap(),
        "--verbose",
    ]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("1 pages"));
}

#[test]
fn refuses_to_overwrite_without_flag() {
    let dir = temp_dir("refuses_to_overwrite");
    let output = dir.join("out.pdf");
    std::fs::write(&output, b"existing").unwrap();
    let input = fixture("libreoffice_table.pdf");
    let result = sumi(&[&input, "-o", output.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(std::fs::read(&output).unwrap(), b"existing");

    let result = sumi(&[&input, "-o", output.to_str().unwrap(), "--overwrite"]);
    assert_eq!(result.status.code(), Some(0));
    assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF-"));
}

#[test]
fn accepts_gray_model() {
    let dir = temp_dir("accepts_gray_model");
    let output = dir.join("out.pdf");
    let input = fixture("libreoffice_table.pdf");
    let out = output.to_str().unwrap();
    let result = sumi(&[&input, "-o", out, "--gray-model", "colorimetric"]);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(std::fs::read(&output).unwrap().starts_with(b"%PDF-"));
    assert_eq!(
        sumi(&[&input, "-o", out, "--overwrite", "--gray-model", "rec601"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn rejects_same_input_and_output() {
    let dir = temp_dir("same_input_output");
    let path = dir.join("in.pdf");
    std::fs::copy(fixture("libreoffice_table.pdf"), &path).unwrap();
    let result = sumi(&[
        path.to_str().unwrap(),
        "-o",
        path.to_str().unwrap(),
        "--overwrite",
    ]);
    assert_eq!(result.status.code(), Some(2));
}

#[test]
fn exit_codes_for_bad_input() {
    let dir = temp_dir("exit_codes");
    let output = dir.join("out.pdf");
    let out = output.to_str().unwrap();

    let garbage = dir.join("garbage.pdf");
    std::fs::write(&garbage, b"this is not a pdf").unwrap();
    assert_eq!(
        sumi(&[garbage.to_str().unwrap(), "-o", out]).status.code(),
        Some(3)
    );

    let missing = dir.join("missing.pdf");
    assert_eq!(
        sumi(&[missing.to_str().unwrap(), "-o", out]).status.code(),
        Some(5)
    );

    let input = fixture("libreoffice_table.pdf");
    assert_eq!(
        sumi(&[&input, "-o", out, "--threshold", "1.5"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        sumi(&[&input, "-o", out, "--mode", "sepia"]).status.code(),
        Some(2)
    );
    assert_eq!(sumi(&[&input]).status.code(), Some(2));
    assert!(!output.exists());
}

#[test]
fn reads_stdin_and_writes_stdout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sumi"))
        .args(["-", "-o", "-", "--mode", "monochrome"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&std::fs::read(fixture("gs_objstm_cmyk.pdf")).unwrap())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.starts_with(b"%PDF-"));
}

fn json_report(result: &Output) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert_eq!(
        stderr.lines().count(),
        1,
        "expected one line of JSON: {stderr}"
    );
    serde_json::from_str(&stderr).unwrap_or_else(|e| panic!("{e}: {stderr}"))
}

#[test]
fn json_report_on_success() {
    let dir = temp_dir("json_report_on_success");
    let output = dir.join("out.pdf");
    // --verbose and the warning would otherwise be printed as text; the JSON replaces both.
    let result = sumi(&[
        &fixture("ycck_jpeg.pdf"),
        "-o",
        output.to_str().unwrap(),
        "--report",
        "json",
        "--verbose",
        "--gray-model",
        "colorimetric",
    ]);
    assert_eq!(result.status.code(), Some(0));
    let report = json_report(&result);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["mode"], "grayscale");
    assert_eq!(report["gray_model"], "colorimetric");
    assert_eq!(report["report"]["pages"], 1);
    assert_eq!(
        report["report"]["warnings"][0]["message"],
        "image not converted: YCCK (Adobe CMYK) JPEG image"
    );
    assert_eq!(report["report"]["warnings"][0]["count"], 1);
    assert!(report["error"].is_null());
}

#[test]
fn json_report_on_failure() {
    let dir = temp_dir("json_report_on_failure");
    let output = dir.join("out.pdf");
    let out = output.to_str().unwrap();

    let garbage = dir.join("garbage.pdf");
    std::fs::write(&garbage, b"this is not a pdf").unwrap();
    let result = sumi(&[garbage.to_str().unwrap(), "-o", out, "--report", "json"]);
    assert_eq!(result.status.code(), Some(3));
    let report = json_report(&result);
    assert_eq!(report["status"], "error");
    assert_eq!(report["exit_code"], 3);
    assert!(report["report"].is_null());
    assert_eq!(report["error"]["kind"], "invalid_pdf");

    let result = sumi(&[
        &fixture("ycck_jpeg.pdf"),
        "-o",
        out,
        "--report",
        "json",
        "--strict",
    ]);
    assert_eq!(result.status.code(), Some(4));
    let report = json_report(&result);
    assert_eq!(report["error"]["kind"], "unsupported");
    assert_eq!(
        report["error"]["details"],
        serde_json::json!(["image not converted: YCCK (Adobe CMYK) JPEG image"])
    );
    assert!(!output.exists());
}

#[test]
fn json_report_on_timeout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sumi"))
        .args(["-", "-o", "-", "--report", "json", "--timeout", "1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Hold standard input open (wait_with_output would close it), so sumi keeps waiting for
    // input until the timeout fires.
    let _stdin = child.stdin.take();
    let result = child.wait_with_output().unwrap();
    assert_eq!(result.status.code(), Some(6));
    let report = json_report(&result);
    assert_eq!(report["exit_code"], 6);
    assert_eq!(report["error"]["kind"], "timeout");
}
