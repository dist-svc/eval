
use std::collections::HashMap;

use async_trait::async_trait;
use clap::Parser;
use futures::{StreamExt, future::join_all};
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

        let mut resps = vec![];

        // Issue request to each peer
        // TODO: parallelise this..?
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

            resps.push(async move {
                resp_rx.await.map(|v| (target_id, v) )
            });
        }

        // Await responses
        let mut resps = join_all(resps).await;

        // Return response collection
        Ok(HashMap::from_iter(resps.drain(..).filter_map(|r| r.ok() )))
    }
}
