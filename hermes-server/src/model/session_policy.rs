use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET/PUT /api/security`: how long a login lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionPolicy {
    /// A login ends this many hours after it was made, however busy the person is.
    pub max_hours: u32,
    /// A login ends after this many minutes without a request from the person; 0 is never.
    pub idle_minutes: u32,
}

impl SessionPolicy {
    pub const DEFAULT: Self = Self {
        max_hours: 14 * 24,
        idle_minutes: 0,
    };
    pub const MAX_HOURS: u32 = 30 * 24;
    pub const MIN_IDLE: u32 = 5;
    pub const MAX_IDLE: u32 = 24 * 60;

    /// Within what the hub accepts: 1 hour to 30 days, and an idle limit of 0 (none) or 5 minutes to a day.
    pub fn valid(&self) -> bool {
        (1..=Self::MAX_HOURS).contains(&self.max_hours)
            && (self.idle_minutes == 0
                || (Self::MIN_IDLE..=Self::MAX_IDLE).contains(&self.idle_minutes))
    }

    pub fn max_ms(&self) -> i64 {
        i64::from(self.max_hours) * 3_600_000
    }

    pub fn idle_ms(&self) -> Option<i64> {
        (self.idle_minutes > 0).then(|| i64::from(self.idle_minutes) * 60_000)
    }
}
