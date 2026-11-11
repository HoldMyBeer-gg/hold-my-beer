//! Where this user's `collab` files live, for the server.
//!
//! The same rule as the CLI's `home` module, repeated here because the
//! server does not depend on the CLI crate: `COLLAB_HOME` if set, otherwise
//! the platform's home directory (`HOME` on Unix, `USERPROFILE` on Windows).
//! The server used to read `HOME` only, so on Windows it never found
//! `~/.collab.toml`.

use std::path::PathBuf;

/// The variable that overrides the home directory for `collab`.
pub const OVERRIDE_VAR: &str = "COLLAB_HOME";

/// The directory `collab` treats as home, or `None` if it cannot tell.
pub fn home_dir() -> Option<PathBuf> {
    resolve(std::env::var(OVERRIDE_VAR).ok(), platform_home())
}

/// Pick between the override and the platform's answer. An override that is
/// empty or only whitespace is ignored, so `COLLAB_HOME=` does not send
/// files to the current directory.
fn resolve(override_dir: Option<String>, platform: Option<PathBuf>) -> Option<PathBuf> {
    match override_dir {
        Some(dir) if !dir.trim().is_empty() => Some(PathBuf::from(dir)),
        _ => platform,
    }
}

#[cfg(windows)]
fn platform_home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var("HOMEDRIVE").ok()?;
            let path = std::env::var("HOMEPATH").ok()?;
            Some(PathBuf::from(format!("{}{}", drive, path)))
        })
}

#[cfg(not(windows))]
fn platform_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<PathBuf> {
        Some(PathBuf::from(s))
    }

    #[test]
    fn the_override_wins_over_the_platform_home() {
        assert_eq!(
            resolve(Some("/tmp/sandbox".into()), p("/home/me")),
            p("/tmp/sandbox")
        );
    }

    #[test]
    fn without_an_override_the_platform_home_is_used() {
        assert_eq!(resolve(None, p("/home/me")), p("/home/me"));
    }

    #[test]
    fn an_empty_override_is_ignored() {
        assert_eq!(resolve(Some(String::new()), p("/home/me")), p("/home/me"));
        assert_eq!(resolve(Some("   ".into()), p("/home/me")), p("/home/me"));
    }

    #[test]
    fn the_override_works_when_the_platform_has_no_home() {
        assert_eq!(resolve(Some("/srv/collab".into()), None), p("/srv/collab"));
    }

    #[test]
    fn nothing_at_all_is_none() {
        assert_eq!(resolve(None, None), None);
    }

    #[test]
    fn a_windows_style_override_is_kept_whole() {
        let dir = r"C:\Users\me\AppData\Local\Temp\.tmpAbC";
        assert_eq!(resolve(Some(dir.into()), p(r"C:\Users\me")), p(dir));
    }
}
