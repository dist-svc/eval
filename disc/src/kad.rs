
use std::collections::HashMap;

use async_trait::async_trait;
use clap::Parser;
use futures::{StreamExt};
use tokio::{sync::{oneshot::{self, Receiver as OneshotReceiver, Sender as OneshotSender}, mpsc::{unbounded_channel, channel, UnboundedReceiver, UnboundedSender}}, select};
use tracing::{trace, debug, info, warn, error};
use tracing_subscriber::{filter::LevelFilter, EnvFilter, FmtSubscriber};
use rand::random;

use dsf_core::prelude::Id;
use kad::{common::{Entry, Request, Response, Error, DatabaseId}, table::{KNodeTable, NodeTable}, dht::{Dht, DhtHandle, RequestSender, RequestReceiver, Net, Connect, SearchOptions}, prelude::DhtConfig, store::HashMapStore};

#[derive(Clone, PartialEq, Debug, Parser)]
pub struct Args {

    #[clap(long, default_value_t = 4)]
    pub peers: usize,

    #[clap(long, default_value_t = 16)]
    pub k: usize,

    #[clap(long, default_value="info")]
    log_level: LevelFilter,
}

type Info = ();
type Data = ();

struct MockPeer {
    dht_handle: DhtHandle<Id, Info, Data>,
    exit_tx: OneshotSender<()>,
}

impl MockPeer {
    pub fn new(id: Id, config: DhtConfig, mut rx: UnboundedReceiver<(Entry<Id, Info>, Request<Id, Data>, OneshotSender<Response<Id, Info, Data>>)>, tx: NetMuxHandle) -> Self {

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
                tokio::select!{
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

        Self{dht_handle, exit_tx}
    }

    fn exit(self) {
        let _ = self.exit_tx.send(());
    }
}

#[derive(Clone)]
struct NetMux {
    mux_ctl: UnboundedSender<MuxCtl>
}

#[derive(Debug)]
enum MuxCtl {
    Register(Id, UnboundedSender<(Entry<Id, Info>, Request<Id, Data>, OneshotSender<Response<Id, Info, Data>>)>),
    Unregister(Id),
    Request(Id, Entry<Id, Info>, Request<Id, Data>, OneshotSender<Response<Id, Info, Data>>),
    Exit,
}

impl NetMux {
    pub fn new() -> Self {

        let (ctl_tx, mut ctl_rx) = unbounded_channel::<MuxCtl>();

        // Start mux task
        tokio::task::spawn(async move {
            let mut peers = HashMap::new();

            loop {
                select!{
                    Some(ctl) = ctl_rx.recv() => {
                        match ctl {
                            MuxCtl::Register(id, req_tx) => {
                                debug!("Register peer: {id:?}");
                                peers.insert(id, req_tx);
                            },
                            MuxCtl::Unregister(id) => {
                                debug!("Deregister peer: {id:?}");
                                peers.remove(&id);
                            }
                            MuxCtl::Request(id, from, req, resp_tx) => {
                                trace!("Mux from: {from:?} to: {id:?} req: {req:?}");
                                match peers.get(&id) {
                                    Some(p) => {
                                        trace!("Mux resp: {p:?}");
                                        p.send((from, req, resp_tx)).unwrap()
                                    },
                                    None => {
                                        warn!("Attempt to send to unregistered peer");
                                    },
                                }
                            }
                            MuxCtl::Exit => break,
                        }
                    }
                }
            }
        });

        // Return mux handle
        Self{ mux_ctl: ctl_tx }
    }

    pub fn handle(&self, id: &Id, peer_tx: UnboundedSender<(Entry<Id, Info>, Request<Id, Data>, OneshotSender<Response<Id, Info, Data>>)>) -> NetMuxHandle {
        // Register handle
        self.mux_ctl.send(MuxCtl::Register(id.clone(), peer_tx)).unwrap();
        // Return handle
        NetMuxHandle { id: id.clone(), mux_ctl: self.mux_ctl.clone() }
    }

    pub fn exit(self) {
        let _ = self.mux_ctl.send(MuxCtl::Exit);
    }
}

#[derive(Clone)]
struct NetMuxHandle {
    id: Id,
    mux_ctl: UnboundedSender<MuxCtl>,
}

impl core::fmt::Debug for NetMuxHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetMuxHandle").field("id", &self.id).finish()
    }
}

#[async_trait]
impl Net<Id, Info, Data> for NetMuxHandle {
    async fn request(
        &self,
        peers: Vec<Entry<Id, Info>>,
        req: Request<Id, Data>,
    ) -> Result<HashMap<Id, Response<Id, Info, Data>>, Error> {

        debug!("Request: {req:?} to peers: {peers:?}");

        let mut resps = HashMap::new();

        // Issue request to each peer
        for p in peers {
            // Send request via mux
            let (resp_tx, resp_rx) = oneshot::channel();
            self.mux_ctl.send(MuxCtl::Request(p.id().clone(), Entry::new(self.id.clone(), ()), req.clone(), resp_tx)).unwrap();

            // Await response
            if let Ok(resp) = resp_rx.await {
                resps.insert(p.id().clone(), resp);
            }
        }

        // Return response collection
        Ok(resps)
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

    let config = DhtConfig::default();

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

    info!("Bootstrapping {} peers", peers.len());
    let bootstrap = peers[0].0.clone();

    for (_id, p) in &mut peers {
        let opts = SearchOptions {
            concurrency: config.concurrency,
            depth: config.max_recursion,
        };

        p.dht_handle.connect(vec![Entry::new(bootstrap.clone(), ())], opts.clone()).await.unwrap();
    }

    info!("Bootstrap complete");

    // TODO: run tests

    // Shutdown peers
    for (_id, p) in peers.drain(..) {
        p.exit();
    }

    // Shutdown mux
    net_mux.exit();

}
