//! Periodic housekeeping: expired challenge values and unused acme-dns accounts.
use std::time::Duration;

use certo_core::User;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
    web::{delete_user_for, sync_txt},
};

/// How long a challenge value stays published after its last write.
pub const CHALLENGE_RETENTION: Duration = Duration::from_secs(2 * 60 * 60);
/// How long an anonymous acme-dns registration may stay unused before it is deleted.
pub const ACMEDNS_ACCOUNT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// What one [`sweep`] removed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// Challenge names whose values expired.
    pub expired_names: usize,
    /// Unused acme-dns accounts deleted.
    pub accounts: usize,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Anonymous acme-dns accounts older than `days` that never published a value.
pub(crate) async fn stale_accounts(state: &AppState, days: u32) -> ApiResult<Vec<User>> {
    let before = now() - i64::from(days) * 86_400;
    state
        .db
        .stale_acmedns_accounts(before, &state.opts.base_domain)
        .await
        .map_err(|e| ApiError::db("stale acme-dns accounts", &e))
}

/// Removes challenge values older than [`CHALLENGE_RETENTION`] and acme-dns accounts
/// unused for [`ACMEDNS_ACCOUNT_RETENTION`]; run hourly and at startup.
pub async fn sweep(state: &AppState) -> Result<SweepReport, String> {
    let before = now() - i64::try_from(CHALLENGE_RETENTION.as_secs()).unwrap_or(i64::MAX);
    let names = state
        .db
        .expire_txt(before)
        .await
        .map_err(|e| format!("expire challenge values: {e}"))?;
    for name in &names {
        sync_txt(state, name).await;
    }
    let days = u32::try_from(ACMEDNS_ACCOUNT_RETENTION.as_secs() / 86_400).unwrap_or(7);
    let unused = stale_accounts(state, days)
        .await
        .map_err(|e| format!("stale acme-dns accounts: {e:?}"))?;
    let mut accounts = 0;
    for user in unused {
        delete_user_for(state, user.id)
            .await
            .map_err(|e| format!("delete {}: {e:?}", user.username))?;
        accounts += 1;
    }
    Ok(SweepReport {
        expired_names: names.len(),
        accounts,
    })
}
