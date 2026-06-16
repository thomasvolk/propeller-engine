// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn platform_log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join("Library/Logs/propeller/propeller.log")
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".local/share/propeller/propeller.log")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        PathBuf::from("/tmp/propeller.log")
    }
}

pub fn init(log_path: &Path) -> WorkerGuard {
    let log_dir = log_path.parent().expect("log path has no parent");
    let log_file = log_path.file_name().expect("log path has no filename");
    std::fs::create_dir_all(log_dir).ok();
    let file_appender = tracing_appender::rolling::never(log_dir, log_file);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
        .init();
    guard
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;
    use tracing_subscriber::prelude::*;

    // Buffer that implements both io::Write and MakeWriter for test assertions.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }
        fn contents(&self) -> Vec<u8> {
            self.0.lock().unwrap().clone()
        }
    }

    impl io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedBuf {
        type Writer = SharedBuf;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn logger_writes_diagnostic_message_to_stderr_layer() {
        let buf = SharedBuf::new();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_writer(buf.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("test diagnostic stderr message");
        });
        let output = String::from_utf8(buf.contents()).unwrap();
        assert!(
            output.contains("test diagnostic stderr message"),
            "expected message in stderr layer output, got: {output}"
        );
    }

    #[test]
    fn logger_writes_diagnostic_message_to_log_file() {
        let dir = tempdir().unwrap();
        let file_appender = tracing_appender::rolling::never(dir.path(), "test.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_writer(non_blocking));
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("test diagnostic file message");
        });
        drop(guard); // flush writer thread
        let content = std::fs::read_to_string(dir.path().join("test.log")).unwrap_or_default();
        assert!(
            content.contains("test diagnostic file message"),
            "expected message in log file, got: {content}"
        );
    }
}
