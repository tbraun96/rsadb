//! A tiny command interpreter standing in for `/system/bin/sh` on the fake device.

use super::FakeConfig;
use std::collections::BTreeMap;

pub struct Exit {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: u8,
}

fn ok(stdout: impl Into<Vec<u8>>) -> Exit {
    Exit {
        stdout: stdout.into(),
        stderr: Vec::new(),
        code: 0,
    }
}

/// Split a command line the way our tests need: whitespace, honouring single quotes.
pub fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut pending = false;
    for c in line.chars() {
        match c {
            '\'' => {
                in_quote = !in_quote;
                pending = true;
            }
            c if c.is_whitespace() && !in_quote => {
                if pending {
                    args.push(std::mem::take(&mut current));
                    pending = false;
                }
            }
            c => {
                current.push(c);
                pending = true;
            }
        }
    }
    if pending {
        args.push(current);
    }
    args
}

pub fn run(config: &FakeConfig, line: &str) -> Exit {
    let args = split_args(line);
    match args.first().map(String::as_str) {
        Some("getprop") => match args.get(1) {
            Some(name) => ok(format!(
                "{}\n",
                config.props.get(name).cloned().unwrap_or_default()
            )),
            None => {
                let lines: Vec<String> = config
                    .props
                    .iter()
                    .map(|(k, v)| format!("[{k}]: [{v}]\n"))
                    .collect();
                ok(lines.concat())
            }
        },
        Some("echo") => ok(format!("{}\n", args[1..].join(" "))),
        Some("true") => ok(""),
        Some("false") => Exit {
            stdout: Vec::new(),
            stderr: Vec::new(),
            code: 1,
        },
        Some("id") => ok("uid=2000(shell) gid=2000(shell)\n"),
        Some("screencap") => ok(config.screencap_png.clone()),
        Some("warn") => Exit {
            stdout: b"out\n".to_vec(),
            stderr: b"err\n".to_vec(),
            code: 3,
        },
        Some("content") => content_query(config, &args),
        Some(other) => Exit {
            stdout: Vec::new(),
            stderr: format!("/system/bin/sh: {other}: inaccessible or not found\n").into_bytes(),
            code: 127,
        },
        None => ok(""),
    }
}

fn content_query(config: &FakeConfig, args: &[String]) -> Exit {
    let projection: Vec<&str> = args
        .iter()
        .position(|a| a == "--projection")
        .and_then(|i| args.get(i + 1))
        .map(|p| p.split(':').collect())
        .unwrap_or_default();
    let mut rows: Vec<&BTreeMap<String, String>> = config.content_rows.iter().collect();
    if projection == ["_id"] {
        // Simulate a provider that reports extra rows to the cross-check.
        rows.extend(config.content_rows.iter().take(config.content_extra_ids));
    }
    if rows.is_empty() {
        return ok("No result found.\n");
    }
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        let fields: Vec<String> = projection
            .iter()
            .map(|col| {
                format!(
                    "{col}={}",
                    row.get(*col).map(String::as_str).unwrap_or("NULL")
                )
            })
            .collect();
        out.push_str(&format!("Row: {i} {}\n", fields.join(", ")));
    }
    ok(out)
}
