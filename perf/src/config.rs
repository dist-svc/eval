use std::time::Duration;
use serde::{Serialize, Deserialize};
use diesel::prelude::*;

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct TestConfig {
    /// Number of client subscribers
    pub num_subscribers: usize,

    /// Number of client publishers
    pub num_publishers: usize,

    /// Size of published messages
    pub message_size: usize,

    /// Period between published messages
    #[serde(with = "humantime_serde")]
    pub publish_period: Duration,

    /// Disable test
    #[serde(default, skip_serializing)]
    pub disabled: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            num_subscribers: 10,
            num_publishers: 10,
            message_size: 32,
            publish_period: Duration::from_secs(1),
            disabled: false,
        }
    }
}
