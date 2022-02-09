
use structopt::StructOpt;

use simplelog::{LevelFilter, SimpleLogger, TermLogger, TerminalMode};
use log::{info, error};

use iot_perf::{Options, Config, run_tests};

#[derive(PartialEq, Clone, Debug, StructOpt)]
pub struct Args {

    #[structopt(flatten)]
    pub options: Options,
    
    #[structopt(long, default_value = "test.toml")]
    /// Test configuration file
    pub config: String,

    #[structopt(long, default_value = "results/")]
    /// Output directory for results
    pub output_dir: String,

    #[structopt(long, default_value = "trace")]
    /// Configure app logging levels (warn, info, debug, trace)
    pub log_level: LevelFilter,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load options
    let opts = Args::from_args();

    // Initialise logging
    let log_config = simplelog::ConfigBuilder::new()
        .add_filter_ignore_str("hyper")
        .add_filter_ignore_str("bollard")
        .build();

    if let Err(_e) = TermLogger::init(opts.log_level, log_config.clone(), TerminalMode::Mixed) {
        SimpleLogger::init(opts.log_level, log_config)?;
    }

    //console_subscriber::init();

    // Load configuration from file
    let config_data = std::fs::read_to_string(&opts.config).expect("error reading configuration file");
    let config: Config = toml::from_str(&config_data).expect("error parsing configuration file");

    // Run tests   
    match run_tests(&opts.options, &config, &opts.output_dir).await {
        Ok(r) => {
            info!("Results: {:?}", r);
            r
        },
        Err(e) => {
            error!("Test error: {:?}", e);
            return Ok(());
        },
    };


    Ok(())
}
