use super::WhenError;

// ─── Lexer ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) enum Tok {
    Bool(bool),
    Null,
    Int(i64),
    Str(String),
    Ident(String),
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq2,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    KwAnd,
    KwOr,
    KwNot,
    KwIn,
    KwMatches,
}

#[allow(clippy::too_many_lines)]
pub(super) fn lex(src: &str) -> Result<Vec<(Tok, usize)>, WhenError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // whitespace
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            i += 1;
            continue;
        }
        let start = i;
        match c {
            b'.' => {
                out.push((Tok::Dot, start));
                i += 1;
            }
            b'(' => {
                out.push((Tok::LParen, start));
                i += 1;
            }
            b')' => {
                out.push((Tok::RParen, start));
                i += 1;
            }
            b'[' => {
                out.push((Tok::LBracket, start));
                i += 1;
            }
            b']' => {
                out.push((Tok::RBracket, start));
                i += 1;
            }
            b',' => {
                out.push((Tok::Comma, start));
                i += 1;
            }
            b'=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Eq2, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '==' (bare '=' is not an operator)".into(),
                    });
                }
            }
            b'!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ne, start));
                    i += 2;
                } else {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "expected '!=' (use 'not' for logical negation)".into(),
                    });
                }
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Le, start));
                    i += 2;
                } else {
                    out.push((Tok::Lt, start));
                    i += 1;
                }
            }
            b'>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    out.push((Tok::Ge, start));
                    i += 2;
                } else {
                    out.push((Tok::Gt, start));
                    i += 1;
                }
            }
            b'"' | b'\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        let esc = bytes[i + 1];
                        let ch = match esc {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'\'' => '\'',
                            _ => {
                                return Err(WhenError::Parse {
                                    pos: i,
                                    message: format!(
                                        "unknown escape \\{} in string literal",
                                        esc as char,
                                    ),
                                });
                            }
                        };
                        s.push(ch);
                        i += 2;
                    } else {
                        s.push(bytes[i] as char);
                        i += 1;
                    }
                }
                if i >= bytes.len() {
                    return Err(WhenError::Parse {
                        pos: start,
                        message: "unterminated string literal".into(),
                    });
                }
                i += 1;
                out.push((Tok::Str(s), start));
            }
            c if c.is_ascii_digit() => {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let num = std::str::from_utf8(&bytes[i..j])
                    .unwrap()
                    .parse::<i64>()
                    .map_err(|e| WhenError::Parse {
                        pos: start,
                        message: format!("invalid integer: {e}"),
                    })?;
                out.push((Tok::Int(num), start));
                i = j;
            }
            c if is_ident_start(c) => {
                let mut j = i;
                while j < bytes.len() && is_ident_cont(bytes[j]) {
                    j += 1;
                }
                let word = &src[i..j];
                let tok = match word {
                    "true" => Tok::Bool(true),
                    "false" => Tok::Bool(false),
                    "null" => Tok::Null,
                    "and" => Tok::KwAnd,
                    "or" => Tok::KwOr,
                    "not" => Tok::KwNot,
                    "in" => Tok::KwIn,
                    "matches" => Tok::KwMatches,
                    _ => Tok::Ident(word.to_string()),
                };
                out.push((tok, start));
                i = j;
            }
            _ => {
                return Err(WhenError::Parse {
                    pos: start,
                    message: format!("unexpected character {:?}", c as char),
                });
            }
        }
    }
    Ok(out)
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_cont(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Closed list of methods callable on `iter`. Adding new ones is
/// a deliberate API extension — typos in user configs surface as
/// "unknown iter method" rather than silently coercing to false.
pub(super) fn is_known_iter_method(name: &str) -> bool {
    matches!(name, "has_file")
}
