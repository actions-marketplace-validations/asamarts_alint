use alint_core::{Error, FixContext, FixOutcome, Fixer, Result, Violation};

/// Strips Unicode bidi control characters (the Trojan Source
/// codepoints U+202A–202E, U+2066–2069) from the file's content.
#[derive(Debug)]
pub struct FileStripBidiFixer;

impl Fixer for FileStripBidiFixer {
    fn describe(&self) -> String {
        "strip Unicode bidi control characters".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        apply_char_filter(
            "bidi",
            "stripped bidi controls from",
            violation,
            ctx,
            crate::no_bidi_controls::is_bidi_control,
            /* preserve_leading_feff = */ false,
        )
    }
}

/// Strips zero-width characters (U+200B / U+200C / U+200D, plus
/// body-internal U+FEFF — a leading BOM is preserved so
/// `no_bom` can own that concern).
#[derive(Debug)]
pub struct FileStripZeroWidthFixer;

impl Fixer for FileStripZeroWidthFixer {
    fn describe(&self) -> String {
        "strip zero-width characters (U+200B/C/D, body-internal U+FEFF)".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        apply_char_filter(
            "zero-width",
            "stripped zero-width chars from",
            violation,
            ctx,
            |c| matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'),
            /* preserve_leading_feff = */ true,
        )
    }
}

/// Strips a leading BOM (UTF-8 / UTF-16 / UTF-32 LE & BE) from
/// the violating file.
#[derive(Debug)]
pub struct FileStripBomFixer;

impl Fixer for FileStripBomFixer {
    fn describe(&self) -> String {
        "strip leading BOM".to_string()
    }

    fn apply(&self, violation: &Violation, ctx: &FixContext<'_>) -> Result<FixOutcome> {
        let Some(path) = &violation.path else {
            return Ok(FixOutcome::Skipped(
                "violation did not carry a path".to_string(),
            ));
        };
        let abs = ctx.root.join(path);
        if ctx.dry_run {
            return Ok(FixOutcome::Applied(format!(
                "would strip BOM from {}",
                path.display()
            )));
        }
        let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
            alint_core::ReadForFix::Bytes(b) => b,
            alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
        };
        let Some(bom) = crate::no_bom::detect_bom(&existing) else {
            return Ok(FixOutcome::Skipped(format!(
                "{} has no BOM",
                path.display()
            )));
        };
        let stripped = &existing[bom.byte_len()..];
        std::fs::write(&abs, stripped).map_err(|source| Error::Io {
            path: abs.clone(),
            source,
        })?;
        Ok(FixOutcome::Applied(format!(
            "stripped {} BOM from {}",
            bom.name(),
            path.display()
        )))
    }
}

/// Shared read-modify-write helper for "remove every char that
/// matches `predicate`" fix ops.
fn apply_char_filter(
    label: &str,
    verb: &str,
    violation: &Violation,
    ctx: &FixContext<'_>,
    predicate: impl Fn(char) -> bool,
    preserve_leading_feff: bool,
) -> Result<FixOutcome> {
    let Some(path) = &violation.path else {
        return Ok(FixOutcome::Skipped(
            "violation did not carry a path".to_string(),
        ));
    };
    let abs = ctx.root.join(path);
    if ctx.dry_run {
        return Ok(FixOutcome::Applied(format!(
            "would strip {label} chars from {}",
            path.display()
        )));
    }
    let existing = match alint_core::read_for_fix(&abs, path, ctx)? {
        alint_core::ReadForFix::Bytes(b) => b,
        alint_core::ReadForFix::Skipped(outcome) => return Ok(outcome),
    };
    let Ok(text) = std::str::from_utf8(&existing) else {
        return Ok(FixOutcome::Skipped(format!(
            "{} is not UTF-8; cannot filter {label} chars",
            path.display()
        )));
    };
    let mut out = String::with_capacity(text.len());
    let mut first_char = true;
    for c in text.chars() {
        let keep_because_leading_bom = preserve_leading_feff && first_char && c == '\u{FEFF}';
        if keep_because_leading_bom || !predicate(c) {
            out.push(c);
        }
        first_char = false;
    }
    if out.as_bytes() == existing {
        return Ok(FixOutcome::Skipped(format!(
            "{} has no {label} chars to strip",
            path.display()
        )));
    }
    std::fs::write(&abs, out.as_bytes()).map_err(|source| Error::Io {
        path: abs.clone(),
        source,
    })?;
    Ok(FixOutcome::Applied(format!("{verb} {}", path.display())))
}
