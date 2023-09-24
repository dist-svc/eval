#![feature(async_closure)]
#![feature(trivial_bounds)]
#![recursion_limit="2048"]

use std::{marker::PhantomData, time::{Duration}, fmt::Debug};
use std::marker::Unpin;

use structopt::StructOpt;
use strum::VariantNames;
use strum_macros::{EnumVariantNames, Display, EnumString};

use futures::future::{self, FutureExt};
use futures::stream::StreamExt;
use tokio::sync::broadcast;

use log::{trace, debug, info, warn, error};
use serde::{Serialize, Deserialize};
use async_timer::oneshot::{Oneshot, Timer};

use bollard::Docker;
use bollard::container::StatsOptions;

use rolling_stats::Stats;

pub mod error;
use crate::error::Error;

pub mod drivers;
use drivers::{Driver, Client, ClientOpts, CoapDriver, MqttDriver, DsfDriver, LoopDriver};

pub mod remote;
use remote::{ContainerStats, DockerMode};

pub mod eval;
use eval::Agent;

mod config;
pub use config::TestConfig;

mod results;
pub use results::Results;

mod schema;

mod helpers;
use helpers::*;

#[cfg(test)]
mod enc;

#[derive(PartialEq, Clone, Debug, StructOpt)]
pub struct Options {
    /// Host for test running
    #[structopt(long)]
    targets: Vec<String>,
    
    /// Test mode filters
    #[structopt(long, possible_values=DriverMode::VARIANTS)]
    mode_filters: Vec<DriverMode>,

    /// Set docker mode
    #[structopt(long, default_value = "http", possible_values=DockerMode::VARIANTS, env)]
    docker_mode: DockerMode,

    /// Set number of test retries
    #[structopt(long, default_value = "3", env)]
    retries: usize,
}


#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    base: Base,
    drivers: Vec<DriverConfig>,
    matrix: Matrix,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Matrix {
    clients: Vec<Clients>,

    frequency: Vec<usize>,

    message_len: usize,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Clients {
    pub publishers: usize,
    pub subscribers: usize,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Range<T> {
    start: T,
    end: T,
    step: T,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Base {
    /// Run time for each test
    #[serde(with = "humantime_serde")]
    runtime: Duration,

    tls_ca: Option<String>,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct DriverConfig {
    /// Driver mode
    pub mode: DriverMode,
    /// Docker container for test server
    pub container: String,
    /// Ports to expose on container
    pub port: u16,
    /// Command to override default
    pub command: Option<String>,
    /// Environmental variables
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Debug, Serialize, Deserialize, EnumString, Display, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
#[serde(rename_all = "lowercase")]
pub enum DriverMode {
    Coap = 1,
    Coaps = 2,
    Mqtt = 3,
    Mqtts = 4,
    Dsf = 5,
    Loop = 6,
}

impl DriverMode {
    pub fn path(&self, host: &str) -> String {
        use DriverMode::*;

        match self {
            Coap => format!("coap://{}:5683", host),
            Coaps => format!("coaps://{}:5684", host),
            Mqtt => format!("tcp://{}:1883", host),
            Mqtts => format!("ssl://{}:8883", host),
            Dsf => format!("{}:10100", host),
            Loop => format!("{}", host),
        }
    }
}



#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub enum SocketKind {
    Tcp,
    Udp,
}

impl std::fmt::Display for SocketKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SocketKind::Tcp => write!(f, "tcp"),
            SocketKind::Udp => write!(f, "udp"),
        }
    }
}


pub async fn run_tests(options: &Options, config: &Config, output_dir: &str) -> Result<Vec<Results>, Error> {
    let mut results = vec![];
    let retries = options.retries;

    // Setup result file name
    let dt: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    let result_file = format!("{output_dir}/results-{}.json", dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));

    // Connect to docker target if enabled
    let mut r = vec![];
    for t in &options.targets {
        if let Some(r1) = remote::setup(&options.docker_mode, t).await? {
            r.push(r1);
        }       
    };

    // For each loaded driver
    for opts in &config.drivers {

        // Check for mode filter
        if (options.mode_filters.len() > 0) && !options.mode_filters.contains(&opts.mode) {
            info!("Skipping test mode: {:?}", opts.mode);
            continue;
        }

        // Launch container if docker is enabled
        let mut index = 0;
        let mut containers = vec![];

        for r in &mut r {
            let (_id, name) = remote::container_start(r, index, &opts.container, opts.command.as_deref(), &[(opts.port, SocketKind::Tcp), (opts.port, SocketKind::Udp)], &opts.env).await?;

            index += 1;
            containers.push(name);
        }
        if r.len() > 0 {
            // Delay to allow containers to start up
            tokio::time::sleep(Duration::from_secs(5)).await;
        }

        // Generate test listing
        let mut tests = vec![];
        let m = &config.matrix;
        for c in &config.matrix.clients {
            for f in &config.matrix.frequency {
                tests.push(TestConfig{
                    num_publishers: c.publishers,
                    num_subscribers: c.subscribers,
                    message_size: m.message_len,
                    publish_period: Duration::from_secs(1) / (*f as u32),
                    disabled: false,
                })
            }
        }

        // And each possible tests
        for t in &tests {

            if t.disabled {
                debug!("Test {:?} disabled", t);
                continue;
            }

            debug!("Test {:?} options: {:?}", t, opts);

            let mut client_opts = ClientOpts {
                use_tls: false,
                tls_ca: config.base.tls_ca.clone(),
                tls_cert: config.base.tls_cert.clone(),
                tls_key: config.base.tls_key.clone(),
            };        

            // Run the test with the appropriate driver
            info!("Starting {} tests", opts.mode);
            let r = match opts.mode {
                DriverMode::Mqtt => {
                    let mut d = MqttDriver::new(&client_opts);
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
                DriverMode::Coap => {
                    let mut d = CoapDriver::new(&client_opts);
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
                DriverMode::Mqtts => {
                    client_opts.use_tls = true;
                    let mut d = MqttDriver::new(&client_opts);
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
                DriverMode::Coaps => {
                    client_opts.use_tls = true;
                    let mut d = CoapDriver::new(&client_opts);
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
                DriverMode::Dsf => {
                    let mut d = DsfDriver::new();
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
                DriverMode::Loop => {
                    let mut d = LoopDriver::new();
                    try_run_test(&mut r, &options.targets, &opts.mode, &mut d, &config.base, t, retries).await
                },
            };
    
            // Handle test results
            match r {
                Ok(v) => results.push(v),
                Err(e) => {
                    error!("Test {:?} error: {:?}", t, e);
                },
            };
        }

        // Shutdown container if docker is enabled
        for i in 0..r.len() {
            let _ = remote::container_stop(&mut r[i], &containers[i]).await;
        }

        // Write results to file

        // Write results to file
        
        info!("Updating result file: '{result_file}'");

        let r = serde_json::to_string(&results)?;
        let o = std::path::Path::new(&result_file);
        if let Some(p) = o.parent() {
            let _ = std::fs::create_dir(p);
        }
        std::fs::write(o, r)?;
    }

    Ok(results)
}

pub async fn try_run_test<D, C>(remote: &mut [Docker], targets: &[String], mode: &DriverMode, driver: &mut D, base: &Base, test: &TestConfig, retries: usize) -> Result<Results, Error>
where
    D: Driver<Client = C> + 'static,
    C: Client + Send + Unpin + Debug + 'static,
{
    let mut i = 0;
    loop {
        match run_test(remote, targets, mode, driver, base, test).await {
            Ok(v) => return Ok(v),
            Err(e) if i < retries - 1 => {
                warn!("Test run failed with error: {:?}", e);
                i += 1;
                continue;
            },
            Err(e) => {
                error!("Test run failed with error: {:?} after {} retries", e, retries);   
                return Err(e);
            }
        }
    }
}

/// Run a single test
pub async fn run_test<D, C>(remote: &mut [Docker], targets: &[String], mode: &DriverMode, driver: &mut D, base: &Base, test: &TestConfig) -> Result<Results, Error>
where
    D: Driver<Client = C> + 'static,
    C: Client + Debug + Send + Unpin + 'static,
{
 
    info!("Initialising test: {:?}", test);

    let session = rand::random();
    let message_size = test.message_size;

    // Setup start and stop channels
    let (start_tx, _start_rx) = broadcast::channel(1);

    let (pub_done_tx, _pub_done_rx) = broadcast::channel(1); 
    let (sub_done_tx, _sub_done_rx) = broadcast::channel(1);


    // Setup subscribers
    debug!("Connecting {} subscribers", test.num_subscribers);

    let sub_clients: Vec<_> = (0..test.num_subscribers).map(|i| {
        let id = format!("test-sub-{}", i);
        let p = mode.path(&targets[i % targets.len()]);
        driver.new( p, i, id)
    }).collect();

    tokio::task::yield_now().await;

    let mut sub_clients = try_join_all_windowed(sub_clients, 10).await?;

    // Setup publishers
    debug!("Connecting {} publishers", test.num_publishers);

    let pub_clients: Vec<_> = (0..test.num_publishers).map(|i| {
        let id = format!("test-pub-{}", i);
        //let p = mode.path(&targets[(i / targets.len()) % targets.len()]);
        //let p = mode.path(&targets[i % targets.len()]);
        let p = mode.path(&targets[0]);
        driver.new(p, i, id)
    }).collect();

    tokio::task::yield_now().await;

    let mut pub_clients = try_join_all_windowed(pub_clients, 10).await?;

    let topics: Vec<_> = pub_clients.iter().map(|c| {
        c.topic()
    }).collect();


    debug!("Setting up publishers");

    let publishers: Vec<_> = FutureWindow::new(
        10,
        pub_clients.drain(..).enumerate(),
        |(i, c)| {

            let topic = topics[i].clone();
            let start_tx = start_tx.subscribe();
            let done_tx = pub_done_tx.subscribe();
            let period = test.publish_period;

            Box::pin(async move {
                Agent::new_publisher(c, start_tx, done_tx, session, topic, message_size, period).await
            })
    }).await;
    let mut publishers = publishers.into_iter().collect::<Result<Vec<Agent<C>>, _>>()?;

    debug!("Setting up subscribers");

    let subscribers: Vec<_> = FutureWindow::new(
        10,
        sub_clients.drain(..).enumerate(),
        |(i, c)| {
            let topic = topics[i % test.num_publishers].clone();
            let start_tx = start_tx.subscribe();
            let done_tx = sub_done_tx.subscribe();

            Box::pin(async move {
                Agent::new_subscriber(c, start_tx, done_tx, session, vec![topic]).await
            })
    }).await;
    let mut subscribers = subscribers.into_iter().collect::<Result<Vec<Agent<C>>, _>>()?;


    info!("Running test");

    // Setup stats collectors
    let mut stats_tasks = vec![];

    for i in 0..remote.len() {
        // Request stats from runtime
        let options = Some(StatsOptions{
            stream: true,
            ..Default::default()
        });
        let container_name = format!("test-{}", i);
        info!("Starting stats collector for: {}", container_name);

        let mut stats_rx = remote[i].stats(&container_name, options).fuse();
        let mut exit = pub_done_tx.subscribe();

        // Setup task to collect em
       
        let h = tokio::spawn(async move {
            let mut stats = ContainerStats::new();

            loop {
                tokio::select! {
                    _ = exit.recv() => {
                        return stats;
                    },
                    s = stats_rx.next() => {
                        if let Some(Ok(s)) = s {
                            if let Some((cpu, mem)) = stats.update(&s) {
                                debug!("Stats - CPU: {:.2}% MEM: {:.2} MB", cpu, mem);
                            }  
                        }
                    },
                }
            }
        });

        stats_tasks.push(h);
    };

    // Start test components
    start_tx.send(()).unwrap();

    // Run test
    let run = Timer::new(base.runtime).fuse();
    run.await;

    // Shutdown test agents, publishers first first
    pub_done_tx.send(()).unwrap();

    // Wait a moment for in-flight requests to clear
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Then subscribers
    sub_done_tx.send(()).unwrap();

    let _ = start_tx;

    // Collect stats
    let mut container_stats: Vec<_> = futures::future::join_all(stats_tasks.drain(..)).await;
    let container_stats: Vec<_> = container_stats.drain(..).filter_map(|v| v.ok() ).collect();

    info!("Test done");

    // Fetch publisher results
    let mut results_pub = futures::future::join_all(publishers.drain(..).map(|s| s.done() )).await;
    let mut results_sub = futures::future::join_all(subscribers.drain(..).map(|s| s.done() )).await;

    trace!("Publisher stats: {:?}", results_pub);
    trace!("Subscriber stats: {:?}", results_sub);

    let cpu_percent = container_stats.iter().map(|s| s.stats().0 ).reduce(|acc, s| acc.merge(&s)).unwrap_or_default();
    let mem_percent = container_stats.iter().map(|s| s.stats().1 ).reduce(|acc, s| acc.merge(&s)).unwrap_or_default();

    let sent = results_pub.drain(..).filter_map(|v| v.ok() ).reduce(|acc, s| acc.merge(&s)).unwrap_or_default();
    let latency = results_sub.drain(..).filter_map(|v| v.ok() ).reduce(|acc, s| acc.merge(&s)).unwrap_or_default();
    
    let packet_loss = 1f64 - ((latency.count as f64) / (sent.count as f64) * (test.num_publishers as f64) / (test.num_subscribers) as f64);

    let throughput = latency.count as f64 / base.runtime.as_secs_f64();


    info!("CPU stats: {:.2} %", cpu_percent);
    info!("MEM stats: {:.2} %", mem_percent);
    info!("PUB stats: {:.2} us", sent);
    info!("SUB stats: {:.2} us", latency);
    info!("Packet loss: {:.2} (sent: {} received: {})", packet_loss, sent.count, latency.count);
    info!("Throughput: {:.2} messages/sec", throughput);

    Ok(Results{
        mode: mode.clone(),
        test: test.clone(),
        latency,
        cpu_percent,
        mem_percent,
        packet_loss,
        throughput,
    })
}


#[cfg(test)]
mod test {
    use super::Config;

    #[test]
    fn test_load_config() {
        let config_data = std::fs::read_to_string("test.toml").expect("error reading configuration file");
        let config: Config = toml::from_str(&config_data).expect("error parsing configuration file");
    }
}


