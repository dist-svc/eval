
use std::{collections::HashMap, sync::{Arc, Mutex}, fmt::Debug};

use async_trait::async_trait;
use clap::Parser;
use futures::{StreamExt, future::join_all};
use rand::random;
use tokio::{
    select,
    sync::{
        mpsc::{channel, unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot::{self, Receiver as OneshotReceiver, Sender as OneshotSender},
    }, task::JoinHandle,
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

mod range;
pub use range::{IntRange, IntRangeIter};

pub type Info = ();
pub type Data = u32;

#[derive(Clone)]
pub struct NetMux {
    mux_ctl: UnboundedSender<MuxCtl>,
}

#[derive(Debug)]
pub enum MuxCtl {
    Register(
        Id,
        UnboundedSender<(
            Entry<Id, Info>,
            Request<Id, Data>,
            OneshotSender<Response<Id, Info, Data>>,
        )>,
    ),
    Unregister(Id),
    Request(
        Id,
        Entry<Id, Info>,
        Request<Id, Data>,
        OneshotSender<Response<Id, Info, Data>>,
    ),
    Exit,
}

impl NetMux {
    pub fn new() -> Self {
        let (ctl_tx, mut ctl_rx) = unbounded_channel::<MuxCtl>();

        // Start mux task
        tokio::task::spawn(async move {
            let mut peers = HashMap::new();

            loop {
                select! {
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
        Self { mux_ctl: ctl_tx }
    }

    pub fn handle(
        &self,
        id: &Id,
        peer_tx: UnboundedSender<(
            Entry<Id, Info>,
            Request<Id, Data>,
            OneshotSender<Response<Id, Info, Data>>,
        )>,
    ) -> NetMuxHandle {
        // Register handle
        self.mux_ctl
            .send(MuxCtl::Register(id.clone(), peer_tx))
            .unwrap();
        // Return handle
        NetMuxHandle {
            id: id.clone(),
            mux_ctl: self.mux_ctl.clone(),
        }
    }

    pub fn exit(self) {
        let _ = self.mux_ctl.send(MuxCtl::Exit);
    }
}

type PeerTable = HashMap<Id, PeerHandle>;

type PeerHandle = UnboundedSender<PeerMessage>;

type PeerMessage = (
    Entry<Id, Info>,
    Request<Id, Data>,
    OneshotSender<Response<Id, Info, Data>>,
);

pub struct NetMux2 {
    peers: Arc<Mutex<PeerTable>>,
}

impl NetMux2 {
    pub fn new() -> Self {
        Self{ peers: Arc::new(Mutex::new(HashMap::new()))}
    }

    pub fn handle(&self, id: &Id, handle: PeerHandle) -> NetMuxHandle2 {
        {
            let mut p = self.peers.lock().unwrap();
            p.insert(id.clone(), handle);
        }

        NetMuxHandle2 {
            id: id.clone(),
            peers: self.peers.clone(),
        }
    }

    pub fn exit(self) {

    }
}

#[derive(Clone)]
pub struct NetMuxHandle2 {
    id: Id,
    peers: Arc<Mutex<PeerTable>>,
}

impl core::fmt::Debug for NetMuxHandle2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetMuxHandle2")
            .field("id", &self.id)
            .finish()
    }
}

#[async_trait]
impl Net<Id, Info, Data> for NetMuxHandle2 {
    async fn request(
        &self,
        peers: Vec<Entry<Id, Info>>,
        req: Request<Id, Data>,
    ) -> Result<HashMap<Id, Response<Id, Info, Data>>, Error> {
        debug!("Request: {req:?} to peers: {peers:?}");

        // Grab handles for each target
        let handles: Vec<_> = {
            let p = self.peers.lock().unwrap();
            let h = peers.iter().filter_map(|e| p.get(e.id()).map(|h| (e.clone(), h.clone())) ).collect();
            drop(p);
            h
        };

        // Issue request to each peer
        let mut resps = vec![];
        for (e, handle) in handles {
            let target_id = e.id().clone();

            // Send request via mux
            let (resp_tx, resp_rx) = oneshot::channel();
            handle.send((
                    Entry::new(self.id.clone(), ()),
                    req.clone(),
                    resp_tx,
                ))
                .unwrap();

            // Collect response futures
            resps.push(async move {
                resp_rx.await.map(|v| (target_id, v) )
            });
        }

        // Await response futures
        let mut resps = join_all(resps).await;

        // Return response collection
        Ok(HashMap::from_iter(resps.drain(..).filter_map(|r| r.ok() )))
    }
}

#[derive(Clone)]
pub struct NetMuxHandle {
    id: Id,
    mux_ctl: UnboundedSender<MuxCtl>,
}

impl core::fmt::Debug for NetMuxHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetMuxHandle")
            .field("id", &self.id)
            .finish()
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

        // Issue request to each peer
        let mut resps = vec![];
        for p in peers {
            let target_id = p.id().clone();
            // Send request via mux
            let (resp_tx, resp_rx) = oneshot::channel();
            self.mux_ctl
                .send(MuxCtl::Request(
                    target_id.clone(),
                    Entry::new(self.id.clone(), ()),
                    req.clone(),
                    resp_tx,
                ))
                .unwrap();

            // Collect response futures
            resps.push(async move {
                resp_rx.await.map(|v| (target_id, v) )
            });
        }

        // Await response futures
        let mut resps = join_all(resps).await;

        // Return response collection
        Ok(HashMap::from_iter(resps.drain(..).filter_map(|r| r.ok() )))
    }
}


pub struct MockPeer<H: Net<Id, Info, Data> + Clone + Debug + 'static> {
    pub dht_handle: DhtHandle<Id, Info, Data>,
    task_handle: JoinHandle<Dht<Id, (), u32, H>>,
    exit_tx: OneshotSender<()>,
}


impl <H: Net<Id, Info, Data> + Clone + Debug> MockPeer<H> {
    pub fn new(
        id: Id,
        config: DhtConfig,
        mut rx: UnboundedReceiver<PeerMessage>,
        tx: H,
    ) -> Self {
        // Setup exit handler
        let (exit_tx, exit_rx) = oneshot::channel();

        // Setup DHT
        let table = KNodeTable::new(id.clone(), config.k, id.max_bits());
        let store = HashMapStore::new();
        let mut dht = Dht::custom(id, config, tx, table, store);
        let dht_handle = dht.get_handle();

        // Start listener task
        let task_handle = tokio::task::spawn(async move {
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
            dht
        });

        Self {
            dht_handle,
            task_handle,
            exit_tx,
        }
    }

    pub async fn exit(self) -> Dht<Id, (), u32, H> {
        let _ = self.exit_tx.send(());
        self.task_handle.await.unwrap()
    }
}
