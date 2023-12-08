#![feature(pin_deref_mut)]

use std::net::{SocketAddr, Ipv4Addr};
use std::task::Poll;
use std::time::{Duration, Instant};

use clap::Parser;
use dsf_core::types::{Id, PageKind};
use dsf_rpc::{ConnectOptions, NsSearchOptions, LocateOptions, RequestKind, DebugCommands, NsCreateOptions};
use futures::{Future, FutureExt};
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

    /// Benchmark configuration file
    #[clap(long, default_value="dsfbench.json")]
    config: String,

    /// Parallelisation factor
    #[clap(short='p', long, default_value="1")]
    parallelise: usize,

    /// Notes for result file
    #[clap(long, default_value="")]
    notes: String,

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
        ns: Option<Id>,

        #[clap(long, default_value_t=default_output())]
        output: String,

        #[clap(short, long, default_value_t = 10)]
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

    #[serde(default="String::new")]
    notes: String,

    #[serde(default="Stats::new")]
    elapsed_overall: Stats<f32>,

    #[serde(default="Stats::new")]
    hops_overall: Stats<f32>,

    elapsed: Vec<Stats<f32>>,

    hops: Vec<Stats<f32>>,
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
    let filter = EnvFilter::from_default_env()
        .add_directive("hyper=warn".parse()?)
        .add_directive("rocket=warn".parse()?)
        .add_directive("dsf_client=warn".parse()?)
        .add_directive(args.log_level.into());
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

            // Setup target list
            let mut targets = vec![];
            for i in (1+args.target_offset)..args.target_count {
                let mut target = args.target.octets();
                target[2] += (i / 256) as u8;
                target[3] += (i % 256) as u8;
                targets.push(Ipv4Addr::from(target));
            }

            let target_port = args.target_port;

            // Run windowing operation
            FutureWindow::new(args.parallelise, targets.iter(), |target| {
                let target = target.clone();
                let target_port = target_port.clone();
                let p = p.clone();
                tokio::task::spawn(async move {
                    // Setup client
                    let mut client = Client::new(format!("http://{}:{}", &target, target_port).as_str()).await.unwrap();
                    
                    // Execute connect
                    let r = client.connect(ConnectOptions{
                        address: p.clone(),
                        id: None,
                        timeout: Duration::from_secs(30).into(),
                    }).await;

                    match r {
                        Ok(v) => info!("Connected {target} {v:?}"),
                        Err(e) => error!("Failed to connect {target}: {e:?}"),
                    }
                })
            }).await;

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
        Mode::NsSearch{ ns, num_services, output } => {
            info!("Starting search test using ns {ns:?} with {} peers", args.target_count);

            let target = format!("http://{}:{}", &args.target, args.target_port);

            // Setup NS if not provided
            let ns = match ns {
                Some(ns) => ns.clone(),
                None => create_ns(&target, "test").await?,
            };

            // Setup services for searching
            info!("Generating {num_services} services");
            let services = create_services(&target, *num_services).await?;

            // Register services with NS
            info!("Registering services with NS");
            register_services(&target, &ns, &services).await?;

            // Setup results tracking
            let mut results = Results{
                ns: ns.clone(),
                services: services.clone(),
                targets: args.target_count,
                notes: args.notes.clone(),
                elapsed: vec![],
                hops: vec![],
                elapsed_overall: Stats::new(),
                hops_overall: Stats::new(),
            };

            // Setup target list
            let targets: Vec<_> = (1..args.target_count)
                .map(|i| {
                    let mut target = args.target.octets();
                    target[2] += (i / 256) as u8;
                    target[3] += i as u8;
                    let target = Ipv4Addr::from(target);
                    target
                }).collect();

            // Execute tests
            let res = FutureWindow::new(args.parallelise, targets.iter(), |target| {
                let services = services.clone();
                let ns = ns.clone();
                let target = target.clone();

                tokio::task::spawn(async move {
                    info!("Searching via {:?} for {} services", target, services.len());

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
    
                    let mut elapsed_stats = rolling_stats::Stats::new();
                    let mut hop_stats = rolling_stats::Stats::new();
    
                    for (i, s) in services.iter().enumerate() {
                        let t1 = Instant::now();
    
                        let s = client.ns_search(NsSearchOptions{
                            ns: ns.clone().into(),
                            name: Some(format!("test-svc-{}", i)),
                            hash: None,
                            options: None,
                            no_persist: true,
                        }).await?;
    
                        let elapsed = Instant::now().duration_since(t1);
                        elapsed_stats.update(elapsed.as_millis() as f32);
                        hop_stats.update(s.info.depth as f32);
    
                        if s.matches.len() == 0 {
                            warn!("No service found for lookup {}", i);
                        }
    
                        debug!("Located {:?} in {}", s, humantime::Duration::from(elapsed));
                    }

                    Result::<_, anyhow::Error>::Ok((elapsed_stats, hop_stats))
                })
            }).await;
                
            // Merge results
            for r in res {
                let (elapsed_stats, hop_stats) = match r {
                    Ok(Ok(v)) => v,
                    _ => continue,
                };

                results.elapsed_overall = results.elapsed_overall.merge(&elapsed_stats);
                results.hops_overall = results.hops_overall.merge(&hop_stats);
                results.elapsed.push(elapsed_stats);
                results.hops.push(hop_stats);
            }

            // Write results file
            let e = serde_json::to_string_pretty(&results)?;
            std::fs::write(output, e.as_bytes())?;

            info!("Search times (ms): {:?}", results.elapsed_overall);
            info!("Hops: {:?}", results.hops_overall);

        }
        Mode::Process { output } => {
            info!("Loading output file: {output:}");

            let b = std::fs::read(output)?;
            let mut results: Results = serde_json::from_slice(&b)?;

            // Process overall results
            results.elapsed_overall = results.elapsed.iter().map(|s| s.clone()).reduce(|acc, s| acc.merge(&s) ).unwrap();
            results.hops_overall = results.hops.iter().map(|s| s.clone()).reduce(|acc, s| acc.merge(&s) ).unwrap();

            info!("Search times (ms): {:?}", results.elapsed_overall);
            info!("Hops: {:?}", results.hops_overall);


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

/// Create a registry service for test use
async fn create_ns(target: &str, name: &str) -> Result<Id, anyhow::Error> {
    // Connect client
    let mut client = Client::new(target).await?;

    let h = client.ns_create(NsCreateOptions{
        name: name.to_string(),
        public: true,
    }).await?;

    debug!("Created NS: {}", h.id);

    Ok(h.id)
}

/// Create a set of services for registering and discovery
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

/// Register provided services using an existing registry
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

pub struct FutureWindow<I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> {
    /// Parallelisation factor
    n: usize,

    /// Iterator over inputs for parallel execution
    inputs: I,

    /// Future to be executed per iterator item
    f: F,

    /// Currently executing futures
    current: Vec<Box<R>>,

    /// Completed executor results
    results: Vec<<R as Future>::Output>,
}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> Unpin for FutureWindow<I, R, F> {}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> FutureWindow<I, R, F> 
where
    R: Unpin,
    <I as Iterator>::Item: core::fmt::Debug,
    <R as Future>::Output: core::fmt::Debug,
{
    pub fn new(n: usize, inputs: I, f: F) -> Self {
        Self {
            n,
            inputs,
            f,
            current: Vec::new(),
            results: Vec::new(),
        }
    }

    fn update(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Vec<<R as Future>::Output>> {
        let mut pending_tasks = true;

        // Ensure we're running n tasks
        while self.current.len() < self.n {
            match self.inputs.next() {
                // If we have remaining values, start tasks
                Some(v) => {
                    debug!("Create task for {v:?}");
                    let f = (self.f)(v);
                    self.current.push(Box::new(f));
                    cx.waker().clone().wake();
                },
                // Otherwise, skip this
                None => {
                    debug!("No pending tasks");
                    pending_tasks = false;
                    break;
                },
            }
        }

        // Poll for completion of current tasks
        let mut current: Vec<_> = self.current.drain(..).collect();

        for mut c in current.drain(..) {
            match c.poll_unpin(cx) {
                Poll::Ready(v) => {
                    // Store result and drop future
                    self.results.push(v);
                },
                Poll::Pending => {
                    // Keep tracking future
                    self.current.push(c);
                },
            }
        }

        // Complete when we have no pending tasks and the current list is empty
        if self.current.is_empty() && !pending_tasks {
            debug!("{} tasks complete", self.results.len());
            Poll::Ready(self.results.drain(..).collect())

        // Force wake if any tasks have completed but we still have some pending
        } else if self.current.len() < self.n && pending_tasks {
            cx.waker().clone().wake();
            Poll::Pending

        } else {
            Poll::Pending
        }
    }
}

impl <I: Iterator, R: Future, F: Fn(<I as Iterator>::Item) -> R> std::future::Future for FutureWindow<I, R, F> 
where
    R: Unpin,
    <I as Iterator>::Item: core::fmt::Debug,
    <R as Future>::Output: core::fmt::Debug,
{
    type Output = Vec<<R as Future>::Output>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
       Self::update(&mut self.as_mut(), cx)
    }
}
