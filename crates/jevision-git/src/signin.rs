//! Signing in to GitHub in the browser with OAuth's device flow, and the one place the token for
//! GitHub comes from: `GH_TOKEN` or `GITHUB_TOKEN` when set (for CI), otherwise the Keychain item
//! the sign-in saved. The token never leaves this crate, and is never written, logged or shown.

use serde_json::Value;

use crate::GitError;
use crate::github::{agent, text};

/// Where people sign in to GitHub.
pub const SITE: &str = "https://github.com";
/// Enough to open pull requests and issues in private repositories, and to push.
const SCOPES: &str = "repo";
const KEYCHAIN_SERVICE: &str = "JEVision GitHub";
const KEYCHAIN_ACCOUNT: &str = "github.com";

/// The code the user enters on GitHub to let JEVision in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    /// Identifies this sign-in when asking GitHub whether it's done.
    pub device_code: String,
    /// What the user types on GitHub, like `WDJB-MJHT`.
    pub user_code: String,
    /// The page where the user types it.
    pub verification_uri: String,
    /// Seconds to wait between asking GitHub whether the user is done.
    pub interval: u64,
    /// Seconds the code stays valid.
    pub expires_in: u64,
}

/// What GitHub said about a sign-in that has started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInStep {
    /// The user hasn't finished on GitHub yet.
    Waiting,
    /// Asked too often: wait this many seconds between the next questions.
    SlowDown { interval: u64 },
    /// Signed in, with this token.
    Token(String),
}

/// GitHub's device flow for one OAuth app.
pub struct DeviceFlow {
    agent: ureq::Agent,
    site: String,
    client_id: String,
}

impl DeviceFlow {
    /// `site` is [`SITE`] except in tests.
    pub fn new(site: &str, client_id: &str) -> Self {
        Self {
            agent: agent(),
            site: site.trim_end_matches('/').to_owned(),
            client_id: client_id.trim().to_owned(),
        }
    }

    /// Asks GitHub for a code for the user to enter.
    pub fn start(&self) -> Result<DeviceCode, GitError> {
        if self.client_id.is_empty() {
            return Err(GitError::SignInUnavailable);
        }
        let answer = self.post(
            "/login/device/code",
            &[("client_id", &self.client_id), ("scope", SCOPES)],
        )?;
        if let Some(error) = answer["error"].as_str() {
            return Err(failure(error, &answer));
        }
        let code = DeviceCode {
            device_code: text(&answer["device_code"]),
            user_code: text(&answer["user_code"]),
            verification_uri: text(&answer["verification_uri"]),
            interval: answer["interval"].as_u64().unwrap_or(5).max(1),
            expires_in: answer["expires_in"].as_u64().unwrap_or(900),
        };
        if code.device_code.is_empty() || code.user_code.is_empty() {
            return Err(unreadable());
        }
        Ok(code)
    }

    /// Asks GitHub once whether the user has entered the code; `interval` is the wait so far.
    pub fn poll(&self, device_code: &str, interval: u64) -> Result<SignInStep, GitError> {
        let answer = self.post(
            "/login/oauth/access_token",
            &[
                ("client_id", &self.client_id),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )?;
        match answer["error"].as_str() {
            None => match answer["access_token"].as_str() {
                Some(token) if !token.is_empty() => Ok(SignInStep::Token(token.to_owned())),
                _ => Err(unreadable()),
            },
            Some("authorization_pending") => Ok(SignInStep::Waiting),
            Some("slow_down") => Ok(SignInStep::SlowDown {
                interval: answer["interval"].as_u64().unwrap_or(0).max(interval + 5),
            }),
            Some(error) => Err(failure(error, &answer)),
        }
    }

    fn post(&self, path: &str, form: &[(&str, &str)]) -> Result<Value, GitError> {
        let mut response = self
            .agent
            .post(&format!("{}{path}", self.site))
            .header("Accept", "application/json")
            .send_form(form.iter().copied())
            .map_err(|_| GitError::Unreachable)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| GitError::Unreachable)?;
        match serde_json::from_str::<Value>(&body) {
            Ok(answer) if answer["error"].is_string() || status == 200 => Ok(answer),
            // An unknown Client ID gets "Not Found".
            _ if status == 404 => Err(GitError::SignInUnavailable),
            _ => Err(GitError::Failed {
                message: format!("GitHub answered with error {status}"),
            }),
        }
    }
}

fn failure(error: &str, answer: &Value) -> GitError {
    match error {
        "expired_token" | "incorrect_device_code" | "token_expired" => GitError::SignInExpired,
        "access_denied" => GitError::SignInDenied,
        "device_flow_disabled"
        | "incorrect_client_credentials"
        | "unsupported_grant_type"
        | "Not Found" => GitError::SignInUnavailable,
        _ => {
            let description = text(&answer["error_description"]);
            GitError::Failed {
                message: if description.is_empty() {
                    error.to_owned()
                } else {
                    description
                },
            }
        }
    }
}

fn unreadable() -> GitError {
    GitError::Failed {
        message: "GitHub sent an answer that can't be read".into(),
    }
}

/// The token for GitHub: `GH_TOKEN` or `GITHUB_TOKEN`, otherwise the one the sign-in saved.
pub fn token() -> Option<String> {
    ["GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|token| token.trim().to_owned())
        .find(|token| !token.is_empty())
        .or_else(keychain::get)
}

/// Keeps the token in the Keychain, replacing the one there.
pub fn save_token(token: &str) -> Result<(), GitError> {
    keychain::set(token)
}

/// Removes the saved token, if any.
pub fn forget_token() -> Result<(), GitError> {
    keychain::delete()
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords;

    use super::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE};
    use crate::GitError;

    /// The Keychain's "item not found".
    const NOT_FOUND: i32 = -25300;

    pub fn get() -> Option<String> {
        let token = passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()?;
        String::from_utf8(token).ok().filter(|t| !t.is_empty())
    }

    pub fn set(token: &str) -> Result<(), GitError> {
        passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, token.as_bytes())
            .map_err(failed)
    }

    pub fn delete() -> Result<(), GitError> {
        match passwords::delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Err(e) if e.code() != NOT_FOUND => Err(failed(e)),
            _ => Ok(()),
        }
    }

    fn failed(e: security_framework::base::Error) -> GitError {
        GitError::Failed {
            message: format!("the Keychain refused: {e}"),
        }
    }
}

/// Elsewhere the token comes only from the environment.
#[cfg(not(target_os = "macos"))]
mod keychain {
    use crate::GitError;

    pub fn get() -> Option<String> {
        None
    }

    pub fn set(_: &str) -> Result<(), GitError> {
        Err(GitError::Failed {
            message: "there's no Keychain on this computer; set GH_TOKEN instead".into(),
        })
    }

    pub fn delete() -> Result<(), GitError> {
        Ok(())
    }
}
