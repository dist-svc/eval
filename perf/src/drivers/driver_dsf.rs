use std::convert::TryFrom;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;
use std::str::FromStr;

use dsf_core::wire::Container;
use futures::{Stream};
use tokio::sync::mpsc::{channel, Sender, Receiver};
use tokio::task::{JoinHandle};
use tokio::time;
use tokio::net::{UdpSocket};

use log::{trace, debug, warn, error};
use async_trait::async_trait;

use dsf_core::prelude::*;
use dsf_core::net::{self, Status};

use crate::error::Error;
use super::{Driver, Client};

#[derive(Clone, PartialEq, Debug)]
pub struct DsfDriver {
}

impl DsfDriver {
    pub fn new() -> Self {
        Self{}
    }
}

#[async_trait]
impl Driver for DsfDriver {
    type Client = DsfClient;

    /// Create a new client using the provided driver
    async fn new(&self, server: String, index: usize, name: String) -> Result<Self::Client, Error> {
        let s = match server.to_socket_addrs().map(|mut a| a.next() ) {
            Ok(Some(a)) => a,
            _ => return Err(Error::Address)
        };
        DsfClient::new(index, name, s).await
    }
}

/// Number of retries for lost messages
const DSF_RETRIES: usize = 1;

/// Enable symmetric crypto
const SYMMETRIC_EN: bool = true;

pub struct DsfClient {
    pub index: usize,
    req_id: u16,

    server: SocketAddr,

    svc_id: Id,

    msg_in_rx: Receiver<Vec<u8>>,
    req_tx: Sender<(u16, SocketAddr, Op, Sender<NetResponse>)>,

    topics: Vec<Id>,

    udp_handle: JoinHandle<Result<(), Error>>,
    exit_tx: Sender<()>,
}

struct Req {
    req_id: u16, 
    addr: SocketAddr,
    op: Op, 
    resp: Sender<NetResponse>,
}

enum Op {
    Req(NetRequest),
    Register,
    Publish(Vec<u8>),
}

impl DsfClient {
   pub async fn new(index: usize, _name: String, server: SocketAddr) -> Result<Self, Error> {
        // Create DSF service
        let mut svc = ServiceBuilder::default().build().unwrap();

        // Generate service page
        let (_n, primary) = svc.publish_primary_buff(Default::default()).unwrap();

        // Bind UDP socket
        let sock = UdpSocket::bind("0.0.0.0:0").await.unwrap();

        let (exit_tx, mut exit_rx) = channel(1);
        let (msg_in_tx, msg_in_rx) = channel(1000);
        let (req_tx, mut req_rx) = channel::<(u16, SocketAddr, Op, Sender<NetResponse>)>(1000);

        let (svc_id, svc_keys) = (svc.id(), svc.keys());

        // Start request task
        let udp_handle = tokio::spawn(async move {
            let mut buff = vec![0u8; 1024];
            let mut peer_id = None;
            let mut keys = HashMap::<Id, Keys>::new();
            let mut rx_handles = HashMap::<u16, Sender<NetResponse>>::new();

            loop {
                tokio::select!(
                    // Handle outgoing requests
                    outgoing = req_rx.recv() => {
                        let (req_id, address, op, resp_ch) = match outgoing {
                            Some(v) => v,
                            None => {
                                error!("Outgoing channel closed");
                                return Err(Error::Unknown)
                            }
                        };
                        
                        let mut req = match op {
                            Op::Req(mut req) => {
                                req.common.public_key = svc_keys.pub_key.clone();
                                req
                            },
                            Op::Register => {
                                let kind = NetRequestKind::Register(svc.id(), vec![Page::try_from(primary.clone()).unwrap()]);
                                let mut req = NetRequest::new(svc.id(), req_id, kind, Flags::CONSTRAINED | Flags::PUB_KEY_REQUEST);
                                req.common.public_key = svc_keys.pub_key.clone();
                                req
                            },
                            Op::Publish(data) => {
                                let opts = DataOptions{body: Some(data.as_ref()), ..Default::default()};
                                let (_n, c) = svc.publish_data_buff(opts).unwrap();
                                let kind = NetRequestKind::PushData(svc.id(), vec![Page::try_from(c).unwrap()]);
                                NetRequest::new(svc.id(), req_id, kind, Flags::CONSTRAINED)
                            },
                        };

                        debug!("Sending request: {:?}", req);

                        // Fetch keying information
                        let enc_key = match peer_id.as_ref().map(|p| keys.get(p) ).flatten() {
                            Some(k) => {
                                if SYMMETRIC_EN {
                                    *req.flags() |= Flags::SYMMETRIC_MODE;
                                }
                                k
                            },
                            None => {
                                *req.flags() |= Flags::PUB_KEY_REQUEST;
                                req.set_public_key(svc.public_key());
                                &svc_keys
                            },
                        };
                        
                        // Encode message
                        let c = match svc.encode_request_buff(&req, enc_key) {
                            Ok(c) => c,
                            Err(e) => {
                                error!("Error encoding message: {:?}", e);
                                return Err(Error::Unknown)
                            }
                        };
                        
                        // Add RX handle
                        rx_handles.insert(req_id, resp_ch);

                        if let Err(e) = sock.send_to(c.raw(), &address).await {
                            error!("UDP send2 error: {:?}", e);
                            return Err(Error::Unknown);
                        }
                    },
                    // Handle incoming messages
                    incoming = sock.recv_from(&mut buff) => {

                        let (n, address) = match incoming {
                            Ok(v) => v,
                            Err(e) => {
                                error!("Incoming socket closed: {:?}", e);
                                return Err(Error::Unknown)
                            }
                        };

                        trace!("Recieve UDP from {}", address);

                        // Parse message (no key / secret stores)
                        let base = match Container::parse(&buff[..n], &keys) {
                            Ok(v) => (v),
                            Err(e) => {
                                error!("DSF parsing error: {:?}", e);
                                continue;
                            }
                        };

                        debug!("Received: {:?}", base);

                        // Store peer ID for later
                        if peer_id.is_none() {
                            peer_id = Some(base.id().clone());
                        }
                        
                        // Convert to network message
                        let req_id = base.header().index();
                        let m = match NetMessage::convert(base, &keys) {
                            Ok(m) => m,
                            Err(_e) => {
                                error!("DSF rx was not a network message");
                                continue;
                            }
                        };
                        
                        // Store symmetric keys on first receipt of public key
                        match (m.pub_key(), keys.contains_key(&m.from())) {
                            (Some(pk), false) => {
                                debug!("Enabling symmetric mode for peer: {:?}", peer_id);
                                let k = svc_keys.derive_peer(pk).unwrap(); 
                                keys.insert(m.from(), k);
                            },
                            _ => (),
                        }

                        // Locate matching request sender
                        let handle = rx_handles.remove(&req_id);
                        trace!("Rx: {:?}", m);

                        // Handle received message
                        match (m, handle) {
                            (NetMessage::Request(req), _) => {

                                // Respond with OK
                                let mut resp = net::Response::new(svc.id(), req_id, net::ResponseKind::Status(net::Status::Ok), Flags::empty());

                                // Fetch keys and enable symmetric mode if available
                                let enc_key = match keys.get(&req.from) {
                                    Some(k) => {
                                        if SYMMETRIC_EN {
                                            *resp.flags() |= Flags::SYMMETRIC_MODE;
                                        }
                                        k
                                    },
                                    None => {
                                        resp.set_public_key(svc.public_key());
                                        &svc_keys
                                    },
                                };
                                                                
                                let c = svc.encode_response_buff(&resp, &enc_key).unwrap();

                                if let Err(e) = sock.send_to(c.raw(), address.clone()).await {
                                    error!("UDP send error: {:?}", e);
                                    return Err(Error::Unknown);
                                }


                                // Handle RX'd data
                                let page = match req.data {
                                    NetRequestKind::PushData(_id, pages) if pages.len() > 0 => pages[0].clone(),
                                    _ => continue,
                                };

                                let data = match page.body() {
                                    Body::Cleartext(v) => v,
                                    _ => {
                                        warn!("Received push with no data");
                                        continue
                                    },
                                };

                                debug!("Received data push: {:?}", data);

                                if let Err(e) = msg_in_tx.send(data.clone()).await {
                                    error!("Message RX send error: {:?}", e);
                                    //break Err(Error::Unknown);
                                    continue;
                                }
                            },
                            (NetMessage::Response(resp), Some(resp_tx)) => {
                                match &resp.data {
                                    net::ResponseKind::ValuesFound(id, pages) => {
                                        for p in pages.iter().filter_map(|p| p.info().pub_key()) {
                                            keys.insert(id.clone(), Keys::new(p));
                                        }
                                    },
                                    _ => (),
                                }

                                // Forward received response to caller
                                if let Err(e) = resp_tx.send(resp).await {
                                    error!("Message TX send error: {:?}", e);
                                    return Err(Error::Unknown);
                                }
                            },
                            _ => (),
                        }
                      
                    },
                    Some(_) = exit_rx.recv() => {
                        debug!("Exiting receive task");
                        drop(sock);
                        return Ok(());
                    }
                )
            }
        });


        let s = Self {
            index,
            server,
            req_id: rand::random(),
            svc_id,
            req_tx,
            msg_in_rx,
            topics: vec![],
            udp_handle,
            exit_tx,
        };

        // Ping endpoint

        Ok(s)
   }


   pub async fn request(&mut self, kind: NetRequestKind) -> Result<NetResponseKind, Error> {
        self.req_id = self.req_id.wrapping_add(1);

        // Build and encode message
        let req = NetRequest::new(self.svc_id.clone(), self.req_id, kind, Flags::CONSTRAINED);

        trace!("Request: {:?}", req); 

        // Generate response channel
        let (tx, mut rx) = channel(1);

        // Transmit request (with retries)
        for _i in 0..DSF_RETRIES {
            // Send request data
            if let Err(_e) = self.req_tx.send((self.req_id, self.server, Op::Req(req.clone()), tx.clone())).await {
                continue;
            }

            // Await response
            match time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(v)) => return Ok(v.data),
                _ => continue,
            }
        }

        Err(Error::Timeout)
   }
}

impl Stream  for DsfClient {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.msg_in_rx).poll_recv(ctx)
    }
}

#[async_trait]
impl Client for DsfClient {

    fn id(&self) -> usize {
        self.index
    }

    fn topic(&self) -> String {
        self.svc_id.to_string()
     }

    // Subscribe to a topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Error> {
        let id = Id::from_str(topic).unwrap();

        debug!("Issuing subscribe request for {}", self.svc_id.to_string());

        // First, locate the service (DHT lookup)
        let resp = self.request(NetRequestKind::Locate(id.clone())).await;
        let _svc = match resp {
            Ok(NetResponseKind::ValuesFound(_id, _pages)) => {
                debug!("Located service: {}", id);
            },
            Err(e) => {
                error!("Locate {}, client error: {:?}", id, e);
                return Err(Error::Unknown)
            }
            _ => panic!("Unhandled locate response: {:?}", resp)
        };

        // Then, subscribe to service
        let resp = self.request(NetRequestKind::Subscribe(id.clone())).await;
        match resp {
            Ok(NetResponseKind::Status(Status::Ok)) => {
                self.topics.push(id.clone());

                debug!("Subscribed to service: {}", id);

                Ok(())
            },
            Ok(v) => {
                error!("subscribe {}, unexpected response: {:?}", id, v);
                Err(Error::Unknown)
            },
            Err(e) => {
                error!("subscribe {}, client error: {:?}", id, e);
                Err(Error::Unknown)
            }
        }
    }


    // Register a topic
    async fn register(&mut self, _topic: &str) -> Result<(), Error> {
        debug!("Issuing register request for {}", self.svc_id.to_string());

        // Request registration
        self.req_id = self.req_id.wrapping_add(1);
        let (tx, mut rx) = channel(1);

        // Transmit request (with retries)
        for _i in 0..DSF_RETRIES {
            // Send request data
            if let Err(_e) = self.req_tx.send((self.req_id, self.server, Op::Register, tx.clone())).await {
                continue;
            }

            // Await response
            match time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(v)) => return Ok(()),
                _ => continue,
            }
        }

        Err(Error::Timeout)
    }

    // Publish data to a topic
    async fn publish(&mut self, _topic: &str, data: &[u8]) -> Result<(), Error> {
        // Request publishing
        self.req_id = self.req_id.wrapping_add(1);
        let (tx, mut rx) = channel(1);

        // Transmit request (with retries)
        for _i in 0..DSF_RETRIES {
            // Send request data
            if let Err(_e) = self.req_tx.send((self.req_id, self.server, Op::Publish(data.to_vec()), tx.clone())).await {
                continue;
            }

            // Await response
            match time::timeout(Duration::from_secs(5), rx.recv()).await {
                Ok(Some(v)) => return Ok(()),
                _ => continue,
            }
        }

        Err(Error::Timeout)
    }

    async fn close(mut self) -> Result<(), Error> {
        for t in self.topics.clone() {
            // Request de-registration
            let resp = self.request(NetRequestKind::Unsubscribe(t.clone())).await;
            match &resp {
                Ok(NetResponseKind::Status(net::Status::Ok)) => {
                    debug!("Deregistered service: {}", t);
                },
                _ => {
                    error!("Error deregistering service (resp: {:?})", resp);
                }
            }
        }

        // Signal for listener to exit
        if let Err(_e) = self.exit_tx.send(()).await {
            error!("Error sending exit signal");
        }

        // Join on handler
        if let Err(e) = self.udp_handle.await {
            error!("DSF client task error: {:?}", e);
        }

        Ok(())
    }
}
