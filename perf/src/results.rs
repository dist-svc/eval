

use diesel::prelude::*;
use chrono::NaiveDateTime;
use serde::{Serialize, Deserialize};
use rolling_stats::Stats;

use crate::{DriverMode, TestConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::tests)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Test {
    pub id: i32,
    pub at: NaiveDateTime,
    pub notes: String,
    //pub results: Vec<Results>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
//#[derive(Queryable, Selectable)]
//#[diesel(table_name = crate::schema::results)]
//#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Results {
    //#[diesel(sql_type = Text)]
    pub mode: DriverMode,
    pub test: TestConfig,

    pub latency: Stats<f64>,
    pub cpu_percent: Stats<f64>,
    pub mem_percent: Stats<f64>,
    pub packet_loss: f64,
    pub throughput: f64,
}
