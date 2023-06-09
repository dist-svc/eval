
use std::net::{SocketAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use clap::Parser;
use dsf_core::types::{Id, PageKind};
use dsf_rpc::{ConnectOptions, NsSearchOptions, LocateOptions, RequestKind, DebugCommands};
use serde::{Serialize, Deserialize};
use tracing::{debug, info, warn, error};
use tracing_subscriber::{filter::LevelFilter, EnvFilter, FmtSubscriber};
use rolling_stats::Stats;

use dsf_client::{Client};
use dsf_rpc::{service::CreateOptions, RegisterOptions, NsRegisterOptions};

#[derive(Clone, Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    mode: Mode,

    /// Target IP for DSF HTTP API
    #[clap(long, default_value="192.168.4.0")]
    target: Ipv4Addr,

    /// Target port for DSF HTTP API
    #[clap(long, default_value="10180")]
    target_port: u16,

    /// Number of targets to exercise (increments lowest byte of target IP)
    #[clap(short='c', long, default_value="1")]
    target_count: usize,

    /// Offset into target list to skip first entries
    #[clap(short='o', long, default_value="0")]
    target_offset: usize,

    /// Benchmark onfiguration file
    #[clap(long, default_value="dsfbench.json")]
    config: String,

    #[clap(long, default_value="info")]
    log_level: LevelFilter,
}

#[derive(Clone, Debug, PartialEq, Parser)]
pub enum Mode {
    /// Bootstrap peers in network
    Bootstrap,
    /// Force all peers to update DHTs
    Update,
    /// Create services and write listing to config file
    CreateServices {
        /// Number of services to create
        #[clap()]
        count: usize,
    },
    /// Register created services in the DHT
    RegisterServices,

    /// Register created services via the specified NS
    NsRegister{
        #[clap(long)]
        ns: Id,
    },

    /// Search for known services via the specified NS
    NsSearch{
        #[clap(long)]
        ns: Id,

        #[clap(long, default_value_t=default_output())]
        output: String,

        #[clap(long)]
        cont: bool,

        #[clap(long, default_value_t = 10)]
        num_services: usize,
    },

    /// Process output file to generate summaries
    Process{
        #[clap(long)]
        output: String,
    }
}

fn default_output() -> String {
    let t: chrono::DateTime<chrono::Utc> = chrono::Utc::now();

    format!("results/test-{}.json", t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
struct Config {
    services: Vec<Id>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Results {
    ns: Id,
    services: Vec<Id>,
    targets: usize,
    
    stats: Vec<Stats<f32>>,

    #[serde(default="Stats::new")]
    overall: Stats<f32>,
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


    // Load or init config
    let mut config = Config::default();
    if let Ok(s) = std::fs::read_to_string(&args.config) {
        debug!("Loading config from {}", args.config);
        config = serde_json::from_str(&s)?;
    }

    match &args.mode {
        Mode::Bootstrap => {
            info!("Bootstrapping {} peers", args.target_count - args.target_offset);

            // Use base peer as target
            let p = SocketAddr::new(args.target.into(), 10100);

            for i in (1+args.target_offset)..args.target_count {
                let mut target = args.target.octets();
                target[2] += (i / 256) as u8;
                target[3] += (i % 256) as u8;
                let target = Ipv4Addr::from(target);

                // Setup client
                let mut client = Client::new(format!("http://{}:{}", &target, args.target_port).as_str()).await.unwrap();
                
                // Execute connect
                let r = client.connect(ConnectOptions{
                    address: p.clone(),
                    id: None,
                    timeout: Duration::from_secs(20).into(),
                }).await;

                match r {
                    Ok(v) => info!("Connected {target} {v:?}"),
                    Err(e) => error!("Failed to connect {target}: {e:?}"),
                }
            }
        }
        Mode::Update => {
            info!("Update {} peers", args.target_count - args.target_offset);

            for i in args.target_offset..args.target_count {

                let mut target = args.target.octets();
                target[2] += (i / 256) as u8;
                target[3] += i as u8;
                let target = Ipv4Addr::from(target);


                info!("Update peer {:?}", target);

                // Connect client
                let mut client = Client::new(format!("http://{}:{}", &target, args.target_port).as_str()).await.unwrap();

                let r = client.request(RequestKind::Debug(DebugCommands::Update)).await;

                match r {
                    Ok(_v) => info!("Updated {target}"),
                    Err(e) => error!("Failed to update {target}: {e:?}"),
                }

            }
        }
        Mode::CreateServices { count } => {
            info!("Creating {count:} services");

            // Connect client
            let mut client = Client::new(format!("http://{}:{}", &args.target, args.target_port).as_str()).await?;

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

            // Connect client
            let mut client = Client::new(format!("http://{}:{}", &args.target, args.target_port).as_str()).await?;

            // Register services
            for s in &config.services {
                let _ = client.register(RegisterOptions{
                    service: s.into(), no_replica: true,
                }).await?;
            }
        },
        Mode::NsRegister { ns } => {
            info!("Registering {} services with NS {:?}", config.services.len(), ns);

            // Connect client
            let mut client = Client::new(format!("http://{}:{}", &args.target, args.target_port).as_str()).await?;

            // Register NS
            let _ = client.register(RegisterOptions { service: ns.into(), no_replica: true }).await?;

            // Register services
            for (i, s) in config.services.iter().enumerate() {
                // Register service in DHT
                let _ = client.register(RegisterOptions { service: s.into(), no_replica: true }).await?;

                // Register service in NS
                let _ = client.ns_register(NsRegisterOptions{
                    ns: ns.into(),
                    target: s.clone(),
                    name: Some(format!("test-svc-{}", i)),
                    options: vec![],
                    hashes: vec![],
                }).await?;
            }
        },
        Mode::NsSearch{ ns, num_services, output, cont } => {
            info!("Starting search test using ns {} with {} peers", ns, args.target_count);

            let mut services = vec![];

            let target = format!("http://{}:{}", &args.target, args.target_port);

            // Setup services for searching
            if !*cont {
                info!("Generating {num_services} services");

                services = create_services(&target, *num_services).await?;

                info!("Registering services with NS");

                register_services(&target, &ns, &services).await?;
            }

            let mut results = Results{
                ns: ns.clone(),
                services: services.clone(),
                targets: args.target_count,
                stats: vec![],
                overall: Stats::new(),
            };

            let mut start = 1;

            if *cont {
                info!("Attempting to continue from {}", output);

                let b = std::fs::read(output)?;
                results = serde_json::from_slice(b.as_slice())?;

                start = results.stats.len() + 1;
            }

            for i in start..args.target_count {

                let mut target = args.target.octets();
                target[2] += (i / 256) as u8;
                target[3] += i as u8;
                let target = Ipv4Addr::from(target);

                info!("Searching via {:?} for {} services", target, config.services.len());

                // Connect client
                let mut client = Client::new(dsf_client::Config{
                    daemon_socket: Some(format!("http://{}:{}", &target, args.target_port)),
                    timeout: Duration::from_secs(20).into(),
                }).await?;

                info!("Looking up NS");

                // Ensure client is aware of this NS
                client.locate(LocateOptions{
                    id: ns.clone(),
                    local_only: false,
                    no_persist: false,
                }).await?;

                info!("Starting searches");

                let mut stats = rolling_stats::Stats::new();

                for (i, s) in services.iter().enumerate() {
                    let t1 = Instant::now();

                    let s = client.ns_search(NsSearchOptions{
                        ns: ns.into(),
                        name: Some(format!("test-svc-{}", i)),
                        hash: None,
                        options: None,
                        no_persist: true,
                    }).await?;

                    let elapsed = Instant::now().duration_since(t1);
                    stats.update(elapsed.as_millis() as f32);

                    if s.len() == 0 {
                        warn!("No service found for lookup {}", i);
                    }

                    debug!("Located {:?} in {}", s, humantime::Duration::from(elapsed));
                }

                results.overall = results.overall.merge(&stats);
                results.stats.push(stats);

                // Update results file
                let e = serde_json::to_string_pretty(&results)?;
                std::fs::write(output, e.as_bytes())?;
            }

            results.overall = results.stats.iter().map(|s| s.clone()).reduce(|acc, s| acc.merge(&s) ).unwrap();

            // Update results file
            let e = serde_json::to_string_pretty(&results)?;
            std::fs::write(output, e.as_bytes())?;

            info!("Search times (ms): {:?}", results.overall);
        }
        Mode::Process { output } => {
            info!("Loading output file: {output:}");

            let b = std::fs::read(output)?;
            let mut results: Results = serde_json::from_slice(&b)?;

            // Back-fill count to balance values when merging
            for s in &mut results.stats[..] {
                s.count = 1;
            }

            // Process overall results
            results.overall = results.stats.iter().map(|s| s.clone()).reduce(|acc, s| acc.merge(&s) ).unwrap();

            info!("Search times (ms): {:?}", results.overall);

            // Update results file
            let e = serde_json::to_string_pretty(&results)?;
            std::fs::write(output, e.as_bytes())?;
        }
    }

    // Write back updated config
    debug!("Write updated config to {}", args.config);
    let c = serde_json::to_string_pretty(&config)?;
    std::fs::write(&args.config, c)?;

    Ok(())
}


async fn create_services(target: &str, count: usize) -> Result<Vec<Id>, anyhow::Error> {
    // Connect client
    let mut client = Client::new(target).await?;

    let mut services = vec![];

    for i in 0..count {
        debug!("Create service {i:}");
        
        let h = client.create(CreateOptions {
            page_kind: Some(PageKind::Generic),
            body: None,
            public: true,
            register: true,
            ..Default::default()
        }).await?;

        services.push(h.id);
    }

    Ok(services)
}

async fn register_services(target: &str, ns: &Id, services: &[Id]) -> Result<(), anyhow::Error> {
    // Connect client
    let mut client = Client::new(target).await?;

    // Register NS
    let _ = client.register(RegisterOptions { service: ns.into(), no_replica: true }).await?;

    // Register services
    for (i, s) in services.iter().enumerate() {
        // Register service in DHT
        let _ = client.register(RegisterOptions { service: s.into(), no_replica: true }).await?;

        // Register service in NS
        let _ = client.ns_register(NsRegisterOptions{
            ns: ns.into(),
            target: s.clone(),
            name: Some(format!("test-svc-{}", i)),
            options: vec![],
            hashes: vec![],
        }).await?;
    }

    Ok(())
}
