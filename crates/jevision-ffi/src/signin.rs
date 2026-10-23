//! Signing in to GitHub: in the browser with a code, or with GitHub CLI's account. The token goes
//! straight to the Keychain; the app only ever sees the account.

use jevision_git::{
    API, DeviceCode, DeviceFlow, GitError, GitHub, GitHubAccount, SITE, SignInStep, access_page,
    forget_token, save_token,
};

use crate::git::git;

#[uniffi::remote(Record)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// What came of asking GitHub whether the user has entered the code.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum GitHubSignInStep {
    /// Not yet: ask again after the same wait.
    Waiting,
    /// Not yet: ask again after this many seconds.
    SlowDown { interval: u64 },
    /// Signed in as this account, and the token kept in the Keychain.
    SignedIn { account: GitHubAccount },
}

/// Starts signing in to GitHub in the browser with the OAuth app `client_id`: the code for the
/// user to enter at the code's page.
#[uniffi::export]
pub fn github_sign_in_start(client_id: String) -> Result<DeviceCode, GitError> {
    DeviceFlow::new(SITE, &client_id).start()
}

/// Asks GitHub once whether the user has entered the code; `interval` is the current wait.
#[uniffi::export]
pub fn github_sign_in_poll(
    client_id: String,
    device_code: String,
    interval: u64,
) -> Result<GitHubSignInStep, GitError> {
    Ok(
        match DeviceFlow::new(SITE, &client_id).poll(&device_code, interval)? {
            SignInStep::Waiting => GitHubSignInStep::Waiting,
            SignInStep::SlowDown { interval } => GitHubSignInStep::SlowDown { interval },
            SignInStep::Token(token) => GitHubSignInStep::SignedIn {
                account: keep(token)?,
            },
        },
    )
}

/// The account GitHub CLI is signed in to, to offer instead of signing in again.
#[uniffi::export]
pub fn github_cli_account() -> Result<GitHubAccount, GitError> {
    GitHub::new(API, git()?.github_cli_token()?).account()
}

/// Signs in with GitHub CLI's account: its token is copied to the Keychain.
#[uniffi::export]
pub fn github_use_cli_account() -> Result<GitHubAccount, GitError> {
    keep(git()?.github_cli_token()?)
}

/// Forgets the token JEVision keeps. GitHub CLI's own sign-in stays as it is.
#[uniffi::export]
pub fn github_sign_out() -> Result<(), GitError> {
    forget_token()
}

/// The page on GitHub where the user takes away the access of the OAuth app `client_id` for
/// good: signing out only forgets the token on this Mac.
#[uniffi::export]
pub fn github_access_page(client_id: String) -> String {
    access_page(&client_id)
}

/// The account of a token GitHub accepts, once the token is in the Keychain.
fn keep(token: String) -> Result<GitHubAccount, GitError> {
    let account = GitHub::new(API, token.clone()).account()?;
    save_token(&token)?;
    Ok(account)
}
