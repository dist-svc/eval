
use std::{time::SystemTime, str::FromStr, num::ParseIntError, fmt::Display};

use clap::Parser;

use rand::random;
use rolling_stats::Stats;
use serde::{Serialize, Deserialize};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{debug, error, info};
use tracing_subscriber::{filter::LevelFilter, EnvFilter, FmtSubscriber};

use dsf_core::prelude::Id;
use kad::{
    common::Entry,
    dht::{Connect, SearchOptions, Store as _, Search as _},
    prelude::DhtConfig, table::NodeTable,
};

use disc::{NetMux, MockPeer, IntRange};

#[derive(Clone, Debug, Parser)]
pub struct Args {
    /// Range of peers for testing
    #[clap(short, long, default_value = "10")]
    pub peers: IntRange,

    /// Range of K values for testing
    #[clap(short, long, default_value = "16")]
    pub k: IntRange,

    /// Range of alpha values for testing
    #[clap(short, long, default_value = "4")]
    pub alpha: IntRange,
    
    #[clap(long, default_value_t = 10)]
    pub entries: usize,

    #[clap(long, default_value = "dht-results.json")]
    pub output: String,

    #[clap(long, default_value = "info")]
    log_level: LevelFilter,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
struct TestResult {
    pub peers: usize,

    pub alpha: usize,

    pub k: usize,

    pub hops_min: f32,

    pub hops_max: f32,

    pub hops_mean: f32,

    pub entries_mean: f32,

    pub errors: usize,

    pub duration_s: usize,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // Parse command line arguments
    let args = Args::parse();

    // Initialise logging
    let filter = EnvFilter::from_default_env().add_directive(args.log_level.into());
    let _ = FmtSubscriber::builder()
        .compact()
        .with_max_level(args.log_level)
        .with_env_filter(filter)
        .try_init();

    debug!("args: {args:?}");

    

    let mut results = vec![];

    for num_peers in args.peers.iter() {
        for alpha in args.alpha.iter() {
            for k in args.k.iter() {
                // Setup DHT config
                let config = DhtConfig{
                    concurrency: alpha,
                    k,
                    max_recursion: 16,
                    ..Default::default()
                };

                // Run test
                let r = run_test(num_peers, args.entries, config).await;
                results.push(r);
            }
        }
    }

    info!("Results: {results:?}");

    let s = if args.output.contains("json") {
        serde_json::to_vec_pretty(&results).unwrap()

    } else if args.output.contains("csv") {
        let mut w = csv::WriterBuilder::new().has_headers(true).from_writer(vec![]);
        for r in &results {
            w.serialize(r).unwrap();
            w.flush().unwrap();
        }
        w.into_inner().unwrap()

    } else {
        error!("Output format not supported {}", args.output);
        return;
    };

    std::fs::write(&args.output, &s).unwrap();

}

async fn run_test(num_peers: usize, num_entries: usize, config: DhtConfig) -> TestResult {
    let net_mux = NetMux::new();

    info!("Start test for config: {config:?}");

    let start = SystemTime::now();

    // Setup mock peers
    info!("Creating {} peers", num_peers);
    let mut peers = vec![];
    for _i in 0..num_peers {
        let id = Id::from(random::<[u8; 32]>());

        let (peer_tx, peer_rx) = unbounded_channel();
        let net_handle = net_mux.handle(&id, peer_tx);

        let p = MockPeer::new(id.clone(), config.clone(), peer_rx, net_handle);

        peers.push((id, p));

    }

    let opts = SearchOptions {
        concurrency: config.concurrency,
        depth: config.max_recursion,
    };

    info!("Bootstrapping {} peers", peers.len());
    let bootstrap = peers[0].0.clone();

    for (_id, p) in &mut peers {
        p.dht_handle
            .connect(vec![Entry::new(bootstrap.clone(), ())], opts.clone())
            .await
            .unwrap();
    }

    // TODO: run tests

    info!("Store {} records", num_entries);
    // Setup entries
    let mut entries = vec![];
    for i in 0..num_entries {
        let id = Id::from(random::<[u8; 32]>());
        let value = i;

        // Write entries to DHT
        peers[0].1.dht_handle.store(id.clone(), vec![value as u32], opts.clone()).await.unwrap();

        entries.push((id, value));
    }

    info!("Search for records");

    // Perform searches and collect statistics
    let mut hop_stats = rolling_stats::Stats::new();
    let mut errors = 0;

    for (_, p) in &peers {
        for (e, _) in &entries {

            let (v, info) = p.dht_handle.search(e.clone(), opts.clone()).await.unwrap();
            if v.len() == 0 {
                errors += 1;
                error!("Failed to retrieve value");
            }

            hop_stats.update(info.depth as f32);
        }
    }

    info!("Search errors: {errors}, hop stats: {hop_stats:?}");

    // Shutdown peers and collect table stats
    let mut table_stats = Stats::new();

    for (_id, p) in peers.drain(..) {
        let dht = p.exit().await;

        let n = dht.nodetable().entries().count();
        table_stats.update(n as f32);
    }

    // Shutdown mux
    net_mux.exit();

    TestResult{
        peers: num_peers,
        alpha: config.concurrency,
        k: config.k,
        errors,
        hops_min: hop_stats.min,
        hops_max: hop_stats.max,
        hops_mean: hop_stats.mean,
        entries_mean: table_stats.mean,
        duration_s: start.elapsed().unwrap().as_secs() as usize ,
    }
}