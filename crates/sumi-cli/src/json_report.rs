//! The machine-readable report printed by `--report json`.
//!
//! The report is written by hand instead of with serde to keep the CLI free of extra
//! dependencies; the schema is small and fixed. Every key is always present (`null` or an
//! empty array when it does not apply), so callers can read fields without checking for them.

use sumi_core::Report;

/// The settings a conversion ran with, echoed back in the report.
pub struct Settings<'a> {
    pub mode: &'a str,
    pub gray_model: &'a str,
}

/// Why the command failed.
pub struct ErrorInfo<'a> {
    pub exit_code: u8,
    /// Stable identifier such as `invalid_pdf` or `timeout`, for callers to branch on.
    pub kind: &'a str,
    /// Human-readable description; its wording may change between versions.
    pub message: &'a str,
    /// Individual problems behind the error (the unconverted parts for `unsupported`).
    pub details: &'a [String],
}

/// Renders the report of a successful conversion as a single line of JSON.
pub fn success(settings: &Settings, report: &Report) -> String {
    render(settings, 0, Some(report), None)
}

/// Renders the report of a failed command as a single line of JSON.
///
/// `report` is the conversion result when the failure happened after converting, such as when
/// writing the output failed.
pub fn failure(settings: &Settings, report: Option<&Report>, error: &ErrorInfo) -> String {
    render(settings, error.exit_code, report, Some(error))
}

fn render(
    settings: &Settings,
    exit_code: u8,
    report: Option<&Report>,
    error: Option<&ErrorInfo>,
) -> String {
    let status = if error.is_some() { "error" } else { "ok" };
    let mut out = String::new();
    out.push_str(&format!(
        "{{\"status\":{},\"exit_code\":{exit_code},\"mode\":{},\"gray_model\":{},\"report\":",
        string(status),
        string(settings.mode),
        string(settings.gray_model)
    ));
    match report {
        Some(report) => push_report(&mut out, report),
        None => out.push_str("null"),
    }
    out.push_str(",\"error\":");
    match error {
        Some(error) => out.push_str(&format!(
            "{{\"kind\":{},\"message\":{},\"details\":{}}}",
            string(error.kind),
            string(error.message),
            strings(error.details)
        )),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn push_report(out: &mut String, report: &Report) {
    let warnings: Vec<String> = report
        .warnings
        .iter()
        .map(|w| {
            format!(
                "{{\"message\":{},\"count\":{}}}",
                string(&w.message),
                w.count
            )
        })
        .collect();
    out.push_str(&format!(
        "{{\"pages\":{},\"content_streams\":{},\"color_operators\":{},\"images\":{},\"shadings\":{},\"warnings\":[{}]}}",
        report.pages,
        report.content_streams,
        report.color_operators,
        report.images,
        report.shadings,
        warnings.join(",")
    ));
}

fn strings(values: &[String]) -> String {
    let items: Vec<String> = values.iter().map(|v| string(v)).collect();
    format!("[{}]", items.join(","))
}

/// Encodes a JSON string literal. Non-ASCII characters are written as UTF-8, which JSON allows.
fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: Settings = Settings {
        mode: "grayscale",
        gray_model: "luma",
    };

    #[test]
    fn escapes_strings() {
        assert_eq!(string("a\"b\\c\nd\u{1}é"), r#""a\"b\\c\nd\u0001é""#);
    }

    #[test]
    fn renders_success() {
        let mut report = Report::default();
        report.pages = 2;
        report.images = 1;
        let json = success(&SETTINGS, &report);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["exit_code"], 0);
        assert_eq!(value["report"]["pages"], 2);
        assert_eq!(value["report"]["images"], 1);
        assert_eq!(value["report"]["warnings"], serde_json::json!([]));
        assert!(value["error"].is_null());
    }

    #[test]
    fn renders_failure_without_report() {
        let details = vec!["JPEG 2000 image".to_string()];
        let error = ErrorInfo {
            exit_code: 4,
            kind: "unsupported",
            message: "unsupported PDF features: JPEG 2000 image",
            details: &details,
        };
        let value: serde_json::Value =
            serde_json::from_str(&failure(&SETTINGS, None, &error)).unwrap();
        assert_eq!(value["status"], "error");
        assert_eq!(value["exit_code"], 4);
        assert!(value["report"].is_null());
        assert_eq!(value["error"]["kind"], "unsupported");
        assert_eq!(
            value["error"]["details"],
            serde_json::json!(["JPEG 2000 image"])
        );
    }
}
