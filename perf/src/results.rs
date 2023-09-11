
use serde::{Serialize, Deserialize};
use rolling_stats::Stats;

use crate::{DriverMode, TestConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Results {
    pub mode: DriverMode,
    pub test: TestConfig,

    pub latency: Stats<f64>,
    pub cpu_percent: Stats<f64>,
    pub mem_percent: Stats<f64>,
    pub packet_loss: f64,
    pub throughput: f64,
}
