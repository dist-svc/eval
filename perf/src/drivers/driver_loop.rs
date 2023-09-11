use std::pin::Pin;
use std::task::{Context, Poll};
use std::collections::{HashMap, hash_map::Entry};

use futures::{Stream};
use log::{debug, error};

use tokio::sync::mpsc::{Receiver, Sender, channel};

use async_trait::async_trait;

use crate::error::Error;
use super::{Driver, Client};

type S = Sender<(String, Vec<u8>)>;

#[derive(Clone, Debug)]
pub struct LoopDriver {
    core_tx: S,
    ctl_tx: Sender<Ctl>,
}

enum Ctl {
    Sub(String, usize, S),
    Unsub(String, usize),
}

impl LoopDriver {
    pub fn new() -> Self {
        let (core_tx, mut core_rx) = channel::<(String, Vec<u8>)>(1000);
        let (ctl_tx, mut ctl_rx) = channel(1000);

        let _handle = tokio::spawn(async move {
            let mut map = HashMap::<String, Vec<(usize, S)>>::new();

            loop {
                tokio::select!(
                    Some((t, v)) = core_rx.recv() => {
                        debug!("Publish {}: {:?}", t, v);
                        if let Some(o) = map.get(&t) {
                            for sub in o.iter() {
                                match sub.1.clone().send((t.clone(), v.clone())).await {
                                    Ok(_) => debug!("Forwarded to: {}", sub.0),
                                    Err(e) => error!("Error forwarding to {}: {:?}", sub.0, e),
                                };
                            }
                        }
                    },
                    Some(c) = ctl_rx.recv() => {
                        match c {
                            Ctl::Sub(t, id, c) => {
                                debug!("Subscribe client {} to '{}'", id, t);
                                match map.entry(t) {
                                    Entry::Occupied(mut o) => {
                                        o.get_mut().retain(|v| v.0 != id);
                                        o.get_mut().push((id, c));
                                    },
                                    Entry::Vacant(v) => {
                                        v.insert(vec![(id, c)]);
                                    },
                                }
                            },
                            Ctl::Unsub(t, id) => {
                                debug!("Unsubscribe client {} from '{}'", id, t);
                                if let Some(o) = map.get_mut(&t) {
                                    o.retain(|v| v.0 != id);
                                }
                            },
                        }
                    },
                    else => {
                        debug!("Loop driver task complete");
                        break;
                    }
                )
            }

            debug!("Exit loop driver");
        });

        Self{
            core_tx,
            ctl_tx,
        }
    }
}

#[async_trait]
impl Driver for LoopDriver {
    type Client = LoopClient;

    /// Create a new client using the provided driver
    async fn new(&self, _server: String, id: usize, _name: String) -> Result<Self::Client, Error> {
        let (tx, rx) = channel(1000);

        Ok(LoopClient{id, rx, tx, 
            ctl_tx: self.ctl_tx.clone(), 
            core_tx: self.core_tx.clone(),
            topics: vec![],
        })
    }
}


pub struct LoopClient {
    pub id: usize,
    topics: Vec<String>,
    
    rx: Receiver<(String, Vec<u8>)>,
    tx: Sender<(String, Vec<u8>)>,

    core_tx: S,
    ctl_tx: Sender<Ctl>,
}

impl std::fmt::Debug for LoopClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopClient").field("id", &self.id).finish()
    }
}

impl Stream  for LoopClient {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(m)) =>Poll::Ready(Some( m.1 )),
            Poll::Ready(None) => Poll::Ready(None),
            _ => Poll::Pending,
        }
    }
}

#[async_trait]
impl Client for LoopClient {

    fn id(&self) -> usize {
        self.id
    }

    fn topic(&self) -> String {
        format!("data-{}", self.id) 
    }

    // Subscribe to a topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Error> {
        self.topics.push(topic.to_string());
        let _ = self.ctl_tx.send(Ctl::Sub(topic.to_string(), self.id, self.tx.clone())).await;
        Ok(())
    }

    // Publish data to a topic
    async fn publish(&mut self, topic: &str, data: &[u8]) -> Result<(), Error> {
        let _ = self.core_tx.send((topic.to_string(), data.to_vec())).await;
        Ok(())
    }

    async fn close(mut self) -> Result<(), Error> {
        for t in self.topics.drain(..) {
            let _ = self.ctl_tx.send(Ctl::Unsub(t, self.id)).await;
        }
        
        Ok(())
    }
}
