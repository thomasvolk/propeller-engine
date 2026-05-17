use std::path::PathBuf;

pub fn resolve() -> PathBuf {
    std::env::var("PROPELLER_SOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/propeller.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_returns_default_when_env_not_set() {
        unsafe { std::env::remove_var("PROPELLER_SOCK") };
        let path = resolve();
        assert_eq!(path, PathBuf::from("/tmp/propeller.sock"));
    }

    #[test]
    fn resolve_uses_env_var_when_set() {
        unsafe { std::env::set_var("PROPELLER_SOCK", "/tmp/custom.sock") };
        let path = resolve();
        assert_eq!(path, PathBuf::from("/tmp/custom.sock"));
        unsafe { std::env::remove_var("PROPELLER_SOCK") };
    }
}
