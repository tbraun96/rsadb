//! Parsing of `content query` output.
//!
//! The `content` tool prints one line per row:
//!
//! ```text
//! Row: 0 _id=12, address=+1555, body=Hello, world
//! ```
//!
//! Values are not escaped, so a value containing `, ` is ambiguous. We
//! resolve columns left to right by looking for `, <next column>=`, which
//! lets the *last* projected column hold anything (put free-text columns last).

use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// One row, keyed by projected column name.
pub type Row = BTreeMap<String, String>;

/// Build the shell command for a query.
pub fn command(uri: &str, projection: &[&str], extra_args: &[&str]) -> String {
    use std::fmt::Write as _;
    let quote = super::shell::quote;
    let mut cmd = format!("content query --uri {}", quote(uri));
    if !projection.is_empty() {
        // Writing to a String cannot fail.
        let _ = write!(cmd, " --projection {}", quote(&projection.join(":")));
    }
    for arg in extra_args {
        cmd.push(' ');
        cmd.push_str(&quote(arg));
    }
    cmd
}

/// Parse `content query` output into rows using the projected column order.
pub fn parse_rows(output: &str, projection: &[&str]) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = Vec::new();
    let last = projection.last().copied();
    for line in output.lines() {
        let line = line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix("Row: ") else {
            // Not a row: either the empty-result notice, or the rest of a value that contained a
            // newline. `content query` prints one row per line and does not escape anything, so a
            // text message written on two lines arrives as two lines -- and dropping the second
            // would silently shorten somebody's message. The projection puts the free-text column
            // last for exactly this reason, so a continuation belongs to that column, newline and
            // all. Found on a real phone: "Text STOP to end msgs." on the line above its sender.
            if line.is_empty() || line == "No result found." {
                continue;
            }
            match (last, rows.last_mut()) {
                (Some(column), Some(row)) => {
                    let value = row.entry(column.to_owned()).or_default();
                    value.push('\n');
                    value.push_str(line);
                }
                _ => return Err(Error::parse(format!("unexpected content line: {line:?}"))),
            }
            continue;
        };
        let (_, fields) = rest
            .split_once(' ')
            .ok_or_else(|| Error::parse(format!("row without fields: {line:?}")))?;
        rows.push(parse_fields(fields, projection)?);
    }
    Ok(rows)
}

fn parse_fields(fields: &str, projection: &[&str]) -> Result<Row> {
    let mut row = Row::new();
    let mut rest = fields;
    for (i, column) in projection.iter().enumerate() {
        let prefix = format!("{column}=");
        rest = rest
            .strip_prefix(&prefix)
            .ok_or_else(|| Error::parse(format!("expected column {column} in {fields:?}")))?;
        let value = match projection.get(i + 1) {
            Some(next) => {
                let delimiter = format!(", {next}=");
                let at = rest
                    .find(&delimiter)
                    .ok_or_else(|| Error::parse(format!("missing column {next} in {fields:?}")))?;
                let value = &rest[..at];
                rest = &rest[at + 2..];
                value
            }
            None => std::mem::take(&mut rest),
        };
        row.insert((*column).to_owned(), value.to_owned());
    }
    Ok(row)
}

/// Count `Row:` lines (for the `_id`-only cross-check).
pub fn count_rows(output: &str) -> usize {
    output.lines().filter(|l| l.starts_with("Row: ")).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_written_on_several_lines_is_kept_whole() {
        // A text message with a newline in it. The continuation is part of the last column, not a
        // broken row: shortening somebody's message would be the worst kind of quiet failure.
        let out = "Row: 0 _id=1, address=+1555, body=Your code is 123.\nText STOP to end msgs.\nRow: 1 _id=2, address=+1666, body=short\n";
        let rows = parse_rows(out, &["_id", "address", "body"]).unwrap_or_default();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["body"], "Your code is 123.\nText STOP to end msgs.");
        assert_eq!(rows[1]["body"], "short");
        // The cross-check counts rows, not lines, so a multi-line body cannot make the counts
        // disagree and fail an honest copy.
        assert_eq!(count_rows(out), 2);
    }

    #[test]
    fn a_continuation_with_no_row_before_it_is_still_an_error() {
        let out = "stray line with no row\n";
        assert!(parse_rows(out, &["_id", "body"]).is_err());
    }

    #[test]
    fn last_column_may_contain_commas() {
        let out = "Row: 0 _id=1, address=+1555, body=Hello, world, bye\nRow: 1 _id=2, address=NULL, body=x\n";
        let rows = parse_rows(out, &["_id", "address", "body"]).unwrap_or_default();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["body"], "Hello, world, bye");
        assert_eq!(rows[1]["address"], "NULL");
        assert_eq!(count_rows(out), 2);
    }

    #[test]
    fn empty_and_malformed() {
        assert!(
            parse_rows("No result found.\n", &["_id"])
                .unwrap_or_default()
                .is_empty()
        );
        assert!(parse_rows("Row: 0 a=1", &["b"]).is_err());
        assert!(parse_rows("garbage", &["a"]).is_err());
        assert!(parse_rows("Row: 0 a=1", &["a", "b"]).is_err());
    }

    #[test]
    fn command_is_quoted() {
        let cmd = command(
            "content://sms/inbox",
            &["_id", "body"],
            &["--where", "read = 0"],
        );
        assert_eq!(
            cmd,
            "content query --uri content://sms/inbox --projection _id:body --where 'read = 0'"
        );
    }
}
