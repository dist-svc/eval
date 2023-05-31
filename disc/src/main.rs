use clap::Parser;
use dsf_core::types::{Id, PageKind};
use serde::{Serialize, Deserialize};
use tracing::{debug, info};
use tracing_subscriber::{filter::LevelFilter, EnvFilter, FmtSubscriber};

use dsf_client::{Client};
use dsf_rpc::{service::CreateOptions, RegisterOptions};

#[derive(Clone, Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    mode: Mode,

    #[clap(flatten)]
    client: dsf_client::Config,

    /// Benchmark onfiguration file
    #[clap(long, default_value="dsfbench.json")]
    config: String,

    #[clap(long, default_value="debug")]
    log_level: LevelFilter,
}

#[derive(Clone, Debug, PartialEq, Parser)]
pub enum Mode {
    /// Create services and write listing to config file
    CreateServices {
        /// Number of services to create
        #[clap()]
        count: usize,
    },
    /// Register created services
    RegisterServices,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
struct Config {
    services: Vec<Id>,
}

impl Default for Config {
    fn default() -> Self {
        Self { services: vec![] }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialise logging
    let filter = EnvFilter::from_default_env().add_directive(args.log_level.into());
    let _ = FmtSubscriber::builder()
        .compact()
        .with_max_level(args.log_level)
        .with_env_filter(filter)
        .try_init();

    // Connect client
    let mut client = Client::new(&args.client).await?;

    // Load or init config
    let mut config = Config::default();
    if let Ok(s) = std::fs::read_to_string(&args.config) {
        debug!("Loading config from {}", args.config);
        config = serde_json::from_str(&s)?;
    }

    match &args.mode {
        Mode::CreateServices { count } => {
            info!("Creating {count:} services");

            for i in 0..*count {
                debug!("Create service {i:}");
                
                let h = client.create(CreateOptions {
                    page_kind: Some(PageKind::Generic),
                    body: None,
                    public: true,
                    register: true,
                    ..Default::default()
                }).await?;

                config.services.push(h.id);
            }
        }
        Mode::RegisterServices => {
            info!("Registering {} services", config.services.len());

            for s in &config.services {
                let _ = client.register(RegisterOptions{
                    service: s.into(), no_replica: true,
                }).await?;
            }
        }
    }

    // Write back updated config
    debug!("Write updated config to {}", args.config);
    let c = serde_json::to_string_pretty(&config)?;
    std::fs::write(&args.config, c)?;

    Ok(())
}
