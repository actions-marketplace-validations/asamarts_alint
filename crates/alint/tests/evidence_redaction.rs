//! W5 leak gate: secret-bearing findings remain present, but no public report
//! surface may serialize their evidence bytes.

use std::path::Path;
use std::process::{Command, Output};

const DOTENV_SECRET: &str = "DOTENV_TOKEN_VALUE_DO_NOT_REPORT";
const PROPERTIES_SECRET: &str = "PROPERTIES_TOKEN_VALUE_DO_NOT_REPORT";
const EXPLICIT_SECRET: &str = "EXPLICIT_TOKEN_VALUE_DO_NOT_REPORT";
const RELATED_PATH_SECRET: &str = "RELATED_PATH_VALUE_DO_NOT_REPORT";
const ENV_PATH_SECRET: &str = "ENV_SELECTOR_PATH_DO_NOT_REPORT";

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("ENV_PATH_SECRET", ENV_PATH_SECRET)
        .output()
        .unwrap()
}

fn all_output(output: &Output) -> Vec<u8> {
    let mut bytes = output.stdout.clone();
    bytes.extend_from_slice(&output.stderr);
    bytes
}

fn assert_redacted(surface: &str, bytes: &[u8]) {
    let rendered = String::from_utf8_lossy(bytes);
    for secret in [
        DOTENV_SECRET,
        PROPERTIES_SECRET,
        EXPLICIT_SECRET,
        RELATED_PATH_SECRET,
        ENV_PATH_SECRET,
    ] {
        assert!(
            !rendered.contains(secret),
            "{surface} leaked {secret}:\n{rendered}"
        );
    }
    let marker_count =
        rendered.matches("<redacted:").count() + rendered.matches("&lt;redacted:").count();
    assert!(
        marker_count >= 3,
        "{surface} dropped one or more redacted findings ({marker_count}/3 markers):\n{rendered}"
    );
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".env"),
        format!("TOKEN={DOTENV_SECRET}\nPATH={RELATED_PATH_SECRET}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app.properties"),
        format!("TOKEN={PROPERTIES_SECRET}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("plain.txt"), EXPLICIT_SECRET).unwrap();
    std::fs::write(
        dir.path().join(format!("{ENV_PATH_SECRET}.txt")),
        "forbidden \n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".scope.env"),
        format!("FILES={ENV_PATH_SECRET}.txt\n"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        format!(
            r#"version: 1
rules:
  - id: dotenv-secret
    kind: dotenv_path_equals
    paths: .env
    path: $.TOKEN
    equals: expected
    level: error
    fix: {{ set_value: {{}} }}
  - id: properties-secret
    kind: properties_path_equals
    paths: app.properties
    path: $.TOKEN
    equals: expected
    level: error
    fix: {{ set_value: {{}} }}
  - id: sensitive-registry-target
    kind: registry_paths_resolve
    source: .env
    extract: {{ dotenv: $.PATH }}
    base: lint_root
    level: error
  - id: env-selector-path
    kind: file_content_forbidden
    paths: "{{{{env.ENV_PATH_SECRET}}}}.txt"
    pattern: forbidden
    level: error
    fix: {{ replace: {{ replacement: allowed }} }}
  - id: sensitive-manifest-path
    kind: no_trailing_whitespace
    paths: "**/*.txt"
    scope_filter:
      include_manifest_paths:
        source: .scope.env
        extract: {{ dotenv: $.FILES }}
    level: error
  - id: explicit-secret
    kind: file_content_forbidden
    paths: plain.txt
    pattern: {EXPLICIT_SECRET}
    message: "explicit evidence: {EXPLICIT_SECRET}"
    sensitive: true
    level: error
"#,
        ),
    )
    .unwrap();
    dir
}

#[test]
fn every_report_baseline_and_fix_surface_is_secret_free() {
    let dir = fixture();
    for format in [
        "human", "json", "sarif", "github", "gitlab", "markdown", "junit", "agent",
    ] {
        let output = run(dir.path(), &["check", ".", "--format", format]);
        assert_eq!(output.status.code(), Some(1), "{format}: {output:?}");
        assert_redacted(format, &all_output(&output));
    }

    for format in ["markdown", "json"] {
        let output = run(dir.path(), &["export-agents-md", "--format", format]);
        assert!(output.status.success(), "{format}: {output:?}");
        let rendered = String::from_utf8_lossy(&output.stdout);
        assert!(
            !rendered.contains(ENV_PATH_SECRET),
            "export-agents-md {format} leaked the selector path:\n{rendered}"
        );
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }

    let baseline = run(
        dir.path(),
        &["baseline", "--output", ".alint-baseline.json", "."],
    );
    assert!(baseline.status.success(), "{baseline:?}");
    let baseline_bytes = std::fs::read(dir.path().join(".alint-baseline.json")).unwrap();
    assert_redacted("baseline", &baseline_bytes);

    for format in ["human", "json"] {
        let output = run(dir.path(), &["fix", "--dry-run", "--format", format, "."]);
        assert_eq!(output.status.code(), Some(1), "fix {format}: {output:?}");
        assert_redacted(&format!("fix {format}"), &all_output(&output));
    }

    for format in ["human", "json"] {
        let fix_dir = fixture();
        let output = run(
            fix_dir.path(),
            &["fix", "--unsafe-fixes", "--format", format, "."],
        );
        assert_eq!(output.status.code(), Some(1), "fix {format}: {output:?}");
        assert_redacted(&format!("fix {format}"), &all_output(&output));
        assert!(
            !std::fs::read_to_string(fix_dir.path().join(".env"))
                .unwrap()
                .contains(DOTENV_SECRET)
        );
        assert!(
            !std::fs::read_to_string(fix_dir.path().join("app.properties"))
                .unwrap()
                .contains(PROPERTIES_SECRET)
        );
        assert!(
            !std::fs::read_to_string(fix_dir.path().join(format!("{ENV_PATH_SECRET}.txt")))
                .unwrap()
                .contains("forbidden"),
            "path redaction must not change the fix target"
        );
    }
}

#[test]
fn sensitive_fix_diff_omits_secret_bearing_file_edits() {
    let dir = fixture();
    let output = run(dir.path(), &["fix", "--diff", "."]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(
        rendered.is_empty(),
        "all previewable edits in this fixture are sensitive and must be omitted:\n{rendered}"
    );
    for secret in [
        DOTENV_SECRET,
        PROPERTIES_SECRET,
        EXPLICIT_SECRET,
        ENV_PATH_SECRET,
    ] {
        assert!(
            !all_output(&output)
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "fix --diff leaked {secret}: {output:?}"
        );
    }
}
