//! LSP 3.17 wire primitives: URIs, UTF-16 positions, ranges, incremental edits,
//! and semantic-token legend definitions.

use contract_syntax::Span;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Semantic token types advertised in `initialize` (indices match the legend).
pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

/// Semantic token modifiers advertised in `initialize` (bit flags).
pub const TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
];

/// Semantic token type indices into [`TOKEN_TYPES`].
#[allow(dead_code)]
pub mod tok {
    pub const NAMESPACE: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const CLASS: u32 = 2;
    pub const STRUCT: u32 = 5;
    pub const PARAMETER: u32 = 7;
    pub const VARIABLE: u32 = 8;
    pub const PROPERTY: u32 = 9;
    pub const ENUM_MEMBER: u32 = 10;
    pub const EVENT: u32 = 11;
    pub const FUNCTION: u32 = 12;
    pub const METHOD: u32 = 13;
    pub const MACRO: u32 = 14;
    pub const KEYWORD: u32 = 15;
    pub const COMMENT: u32 = 17;
    pub const STRING: u32 = 18;
    pub const NUMBER: u32 = 19;
    pub const OPERATOR: u32 = 21;
    pub const DECORATOR: u32 = 22;
}

/// Semantic token modifier bitmasks matching [`TOKEN_MODIFIERS`].
#[allow(dead_code)]
pub mod mods {
    pub const DECLARATION: u32 = 1 << 0;
    pub const READONLY: u32 = 1 << 2;
    pub const MODIFICATION: u32 = 1 << 7;
    pub const DEFAULT_LIBRARY: u32 = 1 << 9;
}

/// LSP `SymbolKind` numeric codes.
#[allow(dead_code)]
pub mod sym_kind {
    pub const MODULE: u32 = 2;
    pub const NAMESPACE: u32 = 3;
    pub const CLASS: u32 = 5;
    pub const METHOD: u32 = 6;
    pub const PROPERTY: u32 = 7;
    pub const FIELD: u32 = 8;
    pub const INTERFACE: u32 = 11;
    pub const FUNCTION: u32 = 12;
    pub const VARIABLE: u32 = 13;
    pub const CONSTANT: u32 = 14;
    pub const STRING: u32 = 15;
    pub const KEY: u32 = 20;
    pub const STRUCT: u32 = 23;
    pub const EVENT: u32 = 24;
}

/// LSP `CompletionItemKind` numeric codes.
#[allow(dead_code)]
pub mod comp_kind {
    pub const METHOD: u32 = 2;
    pub const FUNCTION: u32 = 3;
    pub const FIELD: u32 = 5;
    pub const VARIABLE: u32 = 6;
    pub const CLASS: u32 = 7;
    pub const MODULE: u32 = 9;
    pub const PROPERTY: u32 = 10;
    pub const VALUE: u32 = 12;
    pub const ENUM: u32 = 13;
    pub const KEYWORD: u32 = 14;
    pub const SNIPPET: u32 = 15;
    pub const COLOR: u32 = 16;
    pub const FILE: u32 = 17;
    pub const ENUM_MEMBER: u32 = 20;
    pub const CONSTANT: u32 = 21;
    pub const STRUCT: u32 = 22;
    pub const EVENT: u32 = 23;
}

/// Canonicalize `path` when possible so map keys match across symlinks and
/// relative spellings.
pub fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Convert a filesystem path to a `file://` URI.
pub fn path_to_uri(path: &Path) -> String {
    let norm = normalize_path(path);
    let raw = norm.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(raw.len() + 16);
    encoded.push_str("file://");
    if !raw.starts_with('/') {
        encoded.push('/');
    }
    for &b in raw.as_bytes() {
        let keep =
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~' | b':');
        if keep {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    encoded
}

/// Convert a `file://` URI to a filesystem path.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    // On Windows `file:///C:/foo` has a leading slash before the drive letter.
    let bytes = decoded.as_bytes();
    let trimmed = if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        &decoded[1..]
    } else {
        decoded.as_str()
    };
    Some(normalize_path(Path::new(trimmed)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Return the 0-based line slice of `text` (without trailing `\r`/`\n`).
pub fn line_text(text: &str, line0: u32) -> &str {
    text.split('\n')
        .nth(line0 as usize)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or("")
}

/// Convert a 1-based UTF-8 byte column on `line` to a 0-based LSP UTF-16 offset.
pub fn byte_col_to_utf16(line: &str, col_1based: u32) -> u32 {
    let mut target = (col_1based.saturating_sub(1) as usize).min(line.len());
    while target > 0 && !line.is_char_boundary(target) {
        target -= 1;
    }
    line[..target].encode_utf16().count() as u32
}

/// Convert a 0-based LSP UTF-16 offset on `line` to a 1-based UTF-8 byte column.
pub fn utf16_to_byte_col(line: &str, utf16_col: u32) -> u32 {
    let mut units = 0u32;
    let mut byte_idx = 0usize;
    for ch in line.chars() {
        let len = ch.len_utf16() as u32;
        if units + len > utf16_col {
            break;
        }
        units += len;
        byte_idx += ch.len_utf8();
    }
    byte_idx as u32 + 1
}

/// Convert a Contract [`Span`] (1-based line, 1-based UTF-8 byte cols) into an
/// LSP `Range` JSON value.
pub fn span_to_range(text: &str, span: Span) -> Value {
    if span.line == 0 {
        return range_json(0, 0, 0, 0);
    }
    let line0 = span.line.saturating_sub(1);
    let line = line_text(text, line0);
    let start = byte_col_to_utf16(line, span.col);
    let end = if span.end_col > span.col {
        byte_col_to_utf16(line, span.end_col)
    } else {
        // Point span: extend over the identifier at that byte when possible so
        // diagnostics and highlights remain clearly visible.
        let start_byte = (span.col.saturating_sub(1) as usize).min(line.len());
        let tail = if line.is_char_boundary(start_byte) {
            &line[start_byte..]
        } else {
            ""
        };
        let token_len = tail
            .bytes()
            .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            .count();
        if token_len > 0 {
            byte_col_to_utf16(line, span.col + token_len as u32)
        } else {
            (start + 1).min(line.encode_utf16().count() as u32)
        }
    };
    range_json(line0, start, line0, end.max(start))
}

/// Construct an LSP `Range` JSON value.
pub fn range_json(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
    json!({
        "start": { "line": sl, "character": sc },
        "end": { "line": el, "character": ec },
    })
}

/// Construct an LSP `Location` JSON value.
pub fn location_json(path: &Path, text: &str, span: Span) -> Value {
    json!({
        "uri": path_to_uri(path),
        "range": span_to_range(text, span),
    })
}

/// Whether `(line1, byte_col1)` falls inside `span` (inclusive of end so cursor
/// right after an identifier still resolves it).
pub fn span_contains(span: Span, line1: u32, byte_col1: u32) -> bool {
    if span.line != line1 {
        return false;
    }
    let end = if span.end_col > span.col {
        span.end_col
    } else {
        span.col + 1
    };
    byte_col1 >= span.col && byte_col1 <= end
}

/// Apply LSP `TextDocumentContentChangeEvent` items (full or incremental) to `text`.
pub fn apply_content_changes(text: &mut String, changes: &[Value]) {
    for change in changes {
        let Some(new_text) = change.get("text").and_then(Value::as_str) else {
            continue;
        };
        let Some(range) = change.get("range") else {
            *text = new_text.to_owned();
            continue;
        };
        let Some((sl, sc)) = parse_position(range.get("start")) else {
            *text = new_text.to_owned();
            continue;
        };
        let Some((el, ec)) = parse_position(range.get("end")) else {
            *text = new_text.to_owned();
            continue;
        };
        let start = position_to_byte_offset(text, sl, sc);
        let end = position_to_byte_offset(text, el, ec);
        if start <= end && end <= text.len() {
            text.replace_range(start..end, new_text);
        }
    }
}

/// Parse an LSP `{ line, character }` object.
pub fn parse_position(val: Option<&Value>) -> Option<(u32, u32)> {
    let obj = val?.as_object()?;
    let line = obj.get("line")?.as_u64()? as u32;
    let character = obj.get("character")?.as_u64()? as u32;
    Some((line, character))
}

/// Convert a 0-based `(line, utf16_character)` to a UTF-8 byte offset into `text`.
pub fn position_to_byte_offset(text: &str, line0: u32, utf16_col: u32) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (idx, b) in text.bytes().enumerate() {
        if current_line == line0 {
            break;
        }
        if b == b'\n' {
            current_line += 1;
            line_start = idx + 1;
        }
    }
    if current_line < line0 {
        return text.len();
    }
    let rest = &text[line_start..];
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let line = rest[..line_end]
        .strip_suffix('\r')
        .unwrap_or(&rest[..line_end]);
    let byte_col = utf16_to_byte_col(line, utf16_col).saturating_sub(1) as usize;
    (line_start + byte_col).min(text.len())
}

/// Extract the Contract identifier (including hyphenated and vendor-prefixed names)
/// at `(line0, utf16_col)`, returning `(word, start_col_1based, end_col_1based)`.
pub fn word_at(text: &str, line0: u32, utf16_col: u32) -> Option<(String, u32, u32)> {
    let line = line_text(text, line0);
    if line.is_empty() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut pos = utf16_to_byte_col(line, utf16_col).saturating_sub(1) as usize;
    if pos >= bytes.len() {
        pos = bytes.len().saturating_sub(1);
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    if !is_word(bytes[pos]) && pos > 0 && is_word(bytes[pos - 1]) {
        pos -= 1;
    }
    if !is_word(bytes[pos]) {
        return None;
    }
    let mut start = pos;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos + 1;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    // Trim leading `-` unless it is a vendor prefix (`-exact-`, `-webkit-`, `-apple-`)
    // or CSS custom property (`--foo`).
    let mut slice = &line[start..end];
    if slice.starts_with('-')
        && !slice.starts_with("-exact-")
        && !slice.starts_with("-webkit-")
        && !slice.starts_with("-apple-")
        && !slice.starts_with("--")
    {
        let trim = slice.bytes().take_while(|&b| b == b'-').count();
        start += trim;
        slice = &line[start..end];
    }
    // Trim trailing `-`.
    while slice.ends_with('-') && end > start {
        end -= 1;
        slice = &line[start..end];
    }
    if slice.is_empty() {
        return None;
    }
    Some((slice.to_owned(), start as u32 + 1, end as u32 + 1))
}

/// Full document range from `(0, 0)` to the end of `text`.
pub fn full_document_range(text: &str) -> Value {
    let lines: Vec<&str> = text.split('\n').collect();
    let last_line = lines.len().saturating_sub(1) as u32;
    let last_chars = lines
        .last()
        .map(|l| l.strip_suffix('\r').unwrap_or(l).encode_utf16().count() as u32)
        .unwrap_or(0);
    range_json(0, 0, last_line, last_chars)
}

/// Whether `name` is a valid Contract identifier (non-reserved, valid start/body).
pub fn is_valid_ident(name: &str) -> bool {
    if name.is_empty() || contract_syntax::is_reserved(name) {
        return false;
    }
    let bytes = name.as_bytes();
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_alphanumeric()
            || b == b'_'
            || (b == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic))
        {
            i += 1;
        } else {
            return false;
        }
    }
    true
}
