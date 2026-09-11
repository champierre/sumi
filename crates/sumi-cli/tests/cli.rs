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
