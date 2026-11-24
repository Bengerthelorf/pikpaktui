use super::{fs, interactive};

#[derive(Debug)]
pub struct LoginChallenge(pub String);

impl std::fmt::Display for LoginChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PikPak requires interactive verification")
    }
}
impl std::error::Error for LoginChallenge {}

#[derive(Debug)]
pub(crate) struct LoginSessionSaveError;

impl std::fmt::Display for LoginSessionSaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("failed to save login session")
    }
}

#[derive(Debug)]
pub(crate) enum RefreshError {
    Rejected,
    Unavailable,
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Rejected => "PikPak rejected the refresh token",
            Self::Unavailable => "PikPak token refresh is temporarily unavailable",
        })
    }
}

impl std::error::Error for RefreshError {}

#[derive(Debug)]
pub(crate) struct SignInRequired;

impl std::fmt::Display for SignInRequired {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PikPak sign-in is required")
    }
}

impl std::error::Error for SignInRequired {}

#[cfg(target_os = "android")]
pub(super) fn lock_file_exclusive(file: &fs::File) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::os::fd::AsRawFd;

    // std::fs::File::lock is unsupported on this Android target. The lock
    // still follows the File's lifetime, since closing the descriptor releases it.
    let operation = libc::LOCK_EX
        | if interactive::remaining().is_some() {
            libc::LOCK_NB
        } else {
            0
        };
    interactive::wait_for_lock(|| {
        loop {
            // SAFETY: the descriptor is valid for the duration of the File borrow.
            if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
                return Ok(());
            }
            let error = Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted {
                return Err(error);
            }
        }
    })
}

#[cfg(not(target_os = "android"))]
pub(super) fn lock_file_exclusive(file: &fs::File) -> std::io::Result<()> {
    if interactive::remaining().is_none() {
        return file.lock();
    }
    interactive::wait_for_lock(|| file.try_lock().map_err(Into::into))
}

pub(super) fn refresh_token_rejected(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    ["error", "error_code", "reason", "error_description"]
        .iter()
        .filter_map(|field| value.get(field).and_then(serde_json::Value::as_str))
        .any(|reason| {
            matches!(
                reason.trim().to_ascii_lowercase().as_str(),
                "invalid_grant"
                    | "invalid_refresh_token"
                    | "refresh_token_expired"
                    | "invalid refresh token"
                    | "refresh token expired"
                    | "refresh token is invalid"
                    | "refresh token is expired"
            )
        })
}
