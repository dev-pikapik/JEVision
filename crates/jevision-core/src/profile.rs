//! A separate profile: with `JEVISION_PROFILE=<folder>` the app and the command line keep what
//! they keep for the person — settings, logs, keys, the screen lock — in that folder instead, so
//! a second copy of JEVision, such as the one JEVision's own tests drive, leaves the person's
//! data alone.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variable that names the profile's folder.
pub const PROFILE_VARIABLE: &str = "JEVISION_PROFILE";

/// The profile's folder, from [`PROFILE_VARIABLE`]; `None` for the person's own data.
pub fn profile_folder() -> Option<PathBuf> {
    profile_folder_from(
        std::env::var_os(PROFILE_VARIABLE),
        std::env::current_dir().unwrap_or_default(),
    )
}

/// `value` as an absolute folder: a relative one is in `current`. Trailing slashes are dropped,
/// so the app and the command line name the same profile the same way.
pub fn profile_folder_from(value: Option<OsString>, current: PathBuf) -> Option<PathBuf> {
    let value = PathBuf::from(value.filter(|v| !v.is_empty())?);
    let folder = if value.is_absolute() {
        value
    } else {
        current.join(value)
    };
    Some(folder.components().collect())
}

/// The Keychain service that stands for `service` in `profile`: keys saved in a profile are
/// apart from the person's own, which a profile never reads.
pub fn profile_keychain_service(service: &str, profile: Option<&Path>) -> String {
    match profile {
        Some(folder) => format!("{service} (profile {})", folder.display()),
        None => service.to_owned(),
    }
}

/// [`profile_keychain_service`] in the profile of this process.
pub fn keychain_service(service: &str) -> String {
    profile_keychain_service(service, profile_folder().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_an_absolute_folder_and_unset_or_empty_is_none() {
        let here = PathBuf::from("/work");
        assert_eq!(profile_folder_from(None, here.clone()), None);
        assert_eq!(profile_folder_from(Some("".into()), here.clone()), None);
        assert_eq!(
            profile_folder_from(Some("/tmp/p/".into()), here.clone()),
            Some(PathBuf::from("/tmp/p"))
        );
        assert_eq!(
            profile_folder_from(Some("self/profile".into()), here),
            Some(PathBuf::from("/work/self/profile"))
        );
    }

    #[test]
    fn keys_of_a_profile_live_under_a_service_of_their_own() {
        assert_eq!(
            profile_keychain_service("JEVision GitHub", None),
            "JEVision GitHub"
        );
        assert_eq!(
            profile_keychain_service("JEVision GitHub", Some(Path::new("/tmp/p"))),
            "JEVision GitHub (profile /tmp/p)"
        );
    }
}
