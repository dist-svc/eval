use std::collections::HashMap;

use async_trait::async_trait;
use clap::Parser;
use futures::StreamExt;
use rand::random;
use tokio::{
    select,
    sync::{
        mpsc::{channel, unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot::{self, Receiver as OneshotReceiver, Sender as OneshotSender},
    },
};
use tracing::{debug, error, info, trace, warn};
use tracing_subscriber::{filter::LevelFilter, EnvFilter, FmtSubscriber};

use dsf_core::prelude::Id;
use kad::{
    common::{DatabaseId, Entry, Error, Request, Response},
    dht::{Connect, Dht, DhtHandle, Net, RequestReceiver, RequestSender, SearchOptions, Base, Store as _, Search as _},
    prelude::DhtConfig,
    store::HashMapStore,
    table::{KNodeTable, NodeTable},
};

use disc::{NetMux, NetMuxHandle};

#[derive(Clone, PartialEq, Debug, Parser)]
pub struct Args {
    #[clap(long, default_value_t = 4)]
    pub peers: usize,

    #[clap(long, default_value_t = 4)]
    pub entries: usize,

    #[clap(long, default_value_t = 16)]
    pub k: usize,

    #[clap(long, default_value = "info")]
    log_level: LevelFilter,
}

type Info = ();
type Data = u32;

struct MockPeer {
    dht_handle: DhtHandle<Id, Info, Data>,
    exit_tx: OneshotSender<()>,
}

impl MockPeer {
    pub fn new(
        id: Id,
        config: DhtConfig,
        mut rx: UnboundedReceiver<(
            Entry<Id, Info>,
            Request<Id, Data>,
            OneshotSender<Response<Id, Info, Data>>,
        )>,
        tx: NetMuxHandle,
    ) -> Self {
        // Setup exit handler
        let (exit_tx, exit_rx) = oneshot::channel();

        // Setup DHT
        let table = KNodeTable::new(id.clone(), config.k, id.max_bits());
        let store = HashMapStore::new();
        let mut dht = Dht::custom(id, config, tx, table, store);
        let dht_handle = dht.get_handle();

        // Start listener task
        tokio::task::spawn(async move {
            tokio::pin!(exit_rx);

            debug!("Start DHT task");

            loop {
                tokio::select! {
                    Some((peer, req, resp_tx)) = rx.recv() => {
                        match dht.handle_req(&peer, &req) {
                            Ok(resp) => {
                                resp_tx.send(resp).unwrap()
                            },
                            Err(e) => {
                                error!("Failed to handle request: {e:?}")
                            }
                        }
                    }
                    _ = (&mut dht) => (),
                    _ = (&mut exit_rx) => break,
                }
            }

            debug!("Exit DHT task");
        });

        Self {
            dht_handle,
            exit_tx,
        }
    }

    fn exit(self) {
        let _ = self.exit_tx.send(());
    }
}


#[tokio::main]
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

    let net_mux = NetMux::new();

    let config = DhtConfig{
        max_recursion: 16,
        ..Default::default()
    };

    info!("DHT config: {config:?}");

    // Setup mock peers
    info!("Creating {} peers", args.peers);
    let mut peers = vec![];
    for _i in 0..args.peers {
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

    info!("Store {} records", args.entries);
    // Setup entries
    let mut entries = vec![];
    for i in 0..args.entries {
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

    // Shutdown peers
    for (_id, p) in peers.drain(..) {
        p.exit();
    }

    // Shutdown mux
    net_mux.exit();
}
