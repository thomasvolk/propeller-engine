// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::path::PathBuf;

pub fn resolve(env_var: &str, default: &str) -> PathBuf {
    std::env::var(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialise tests that mutate process-wide env vars.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_returns_default_when_env_not_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("PROPELLER_TEST_SOCK") };
        let path = resolve("PROPELLER_TEST_SOCK", "/tmp/propeller-test.sock");
        assert_eq!(path, PathBuf::from("/tmp/propeller-test.sock"));
    }

    #[test]
    fn resolve_uses_env_var_when_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("PROPELLER_TEST_SOCK", "/tmp/custom.sock") };
        let path = resolve("PROPELLER_TEST_SOCK", "/tmp/propeller-test.sock");
        assert_eq!(path, PathBuf::from("/tmp/custom.sock"));
        unsafe { std::env::remove_var("PROPELLER_TEST_SOCK") };
    }
}
