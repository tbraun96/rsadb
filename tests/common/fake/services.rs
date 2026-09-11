//! Service handlers the fake device can open.

use super::shell;
use super::sync::SyncHandler;
use super::{FakeConfig, Shared};
use bytes::Bytes;
use rsadb::services::shell_v2::{self, FrameId};

/// Device side of one stream: produces output on open and in reply to data.
pub trait ServiceHandler: Send {
    fn on_open(&mut self) -> Vec<Bytes>;
    fn on_data(&mut self, data: &[u8]) -> Vec<Bytes>;
    fn finished(&self) -> bool;
}

/// Emits fixed bytes then closes.
pub struct Oneshot {
    output: Vec<Bytes>,
}

impl ServiceHandler for Oneshot {
    fn on_open(&mut self) -> Vec<Bytes> {
        std::mem::take(&mut self.output)
    }
    fn on_data(&mut self, _: &[u8]) -> Vec<Bytes> {
        Vec::new()
    }
    fn finished(&self) -> bool {
        true
    }
}

/// `shell,v2:` with stdin support for `cat`.
pub struct ShellV2 {
    config: std::sync::Arc<FakeConfig>,
    command: String,
    parser: shell_v2::Parser,
    stdin: Vec<u8>,
    done: bool,
}

impl ShellV2 {
    fn finish(&mut self) -> Vec<Bytes> {
        self.done = true;
        let exit = if self.command == "cat" {
            shell::Exit {
                stdout: std::mem::take(&mut self.stdin),
                stderr: Vec::new(),
                code: 0,
            }
        } else {
            shell::run(&self.config, &self.command)
        };
        let mut frames = Vec::new();
        if !exit.stdout.is_empty() {
            frames.push(shell_v2::encode(FrameId::Stdout, &exit.stdout).unwrap());
        }
        if !exit.stderr.is_empty() {
            frames.push(shell_v2::encode(FrameId::Stderr, &exit.stderr).unwrap());
        }
        frames.push(shell_v2::encode(FrameId::Exit, &[exit.code]).unwrap());
        frames
    }
}

impl ServiceHandler for ShellV2 {
    fn on_open(&mut self) -> Vec<Bytes> {
        Vec::new()
    }

    fn on_data(&mut self, data: &[u8]) -> Vec<Bytes> {
        let frames = self.parser.push(data).unwrap_or_default();
        for frame in frames {
            match frame.id {
                FrameId::Stdin => self.stdin.extend_from_slice(&frame.data),
                FrameId::CloseStdin => return self.finish(),
                _ => {}
            }
        }
        Vec::new()
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// Build the handler for `service`, or `None` to refuse the open.
pub fn open(shared: &Shared, service: &str) -> Option<Box<dyn ServiceHandler>> {
    let config = &shared.config;
    if config.refuse_services.iter().any(|s| s == service) {
        return None;
    }
    if let Some(cmd) = service.strip_prefix("shell,v2:") {
        return Some(Box::new(ShellV2 {
            config: std::sync::Arc::clone(config),
            command: cmd.to_owned(),
            parser: shell_v2::Parser::default(),
            stdin: Vec::new(),
            done: false,
        }));
    }
    if let Some(cmd) = service.strip_prefix("shell:") {
        let exit = shell::run(config, cmd);
        let mut merged = exit.stdout;
        merged.extend_from_slice(&exit.stderr);
        return Some(Box::new(Oneshot {
            output: vec![Bytes::from(merged)],
        }));
    }
    if let Some(cmd) = service.strip_prefix("exec:") {
        let exit = shell::run(config, cmd);
        return Some(Box::new(Oneshot {
            output: vec![Bytes::from(exit.stdout)],
        }));
    }
    if service == "sync:" {
        return Some(Box::new(SyncHandler::new(std::sync::Arc::clone(
            &shared.fs,
        ))));
    }
    if let Some(target) = service.strip_prefix("reboot:") {
        shared.rebooted.lock().unwrap().push(target.to_owned());
        return Some(Box::new(Oneshot { output: Vec::new() }));
    }
    None
}
