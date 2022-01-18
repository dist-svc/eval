
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use log::{debug, error};

use futures::{Stream, StreamExt};

use async_trait::async_trait;

use coap_client::{TokioClient as CoapClientAsync, RequestOptions, backend::TokioObserve};

use crate::error::Error;
use super::{Driver, Client, ClientOpts};

#[derive(Clone, PartialEq, Debug)]
pub struct CoapDriver {
    opts: coap_client::ClientOptions,
}

impl CoapDriver {
    pub fn new(opts: &ClientOpts) -> Self {
        let coap_opts = coap_client::ClientOptions {
            connect_timeout: Duration::from_secs(30),
            tls_ca: opts.tls_ca.clone(),
            tls_cert: opts.tls_cert.clone(),
            tls_key: opts.tls_key.clone(),
            tls_skip_verify: true,
        };

        Self{ opts: coap_opts }
    }
}


#[async_trait]
impl Driver for CoapDriver {
    type Client = CoapClient;

    /// Create a new client using the provided driver
    async fn new(&self, server: String, id: usize, _name: String) -> Result<Self::Client, Error> {
        let client = CoapClientAsync::connect(server.as_str(), &self.opts).await?;

        Ok(CoapClient{id, client, subs: vec![]})
    }
}


pub struct CoapClient {
    pub id: usize,
    client: CoapClientAsync,
    subs: Vec<TokioObserve>,
}

impl Stream for CoapClient {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        for i in 0..self.subs.len() {
            let p = match self.subs[i].poll_next_unpin(cx) {
                Poll::Ready(Some(r)) => r,
                Poll::Ready(_) => return Poll::Ready(None),
                Poll::Pending => continue,
            };

            match p {
                Ok(m) => {
                    // Ensure we re-wake if there are other subs to poll
                    if i < self.subs.len() - 1 {
                        cx.waker().clone().wake();
                    }

                    return Poll::Ready(Some(m.payload))
                }
                Err(e) => {
                    // TODO: return data / error
                    error!("Poll error: {:?}", e);
                }
            };

        }

        Poll::Pending
    }
}



#[async_trait]
impl Client for CoapClient {

    fn id(&self) -> usize {
        self.id
    }

    fn topic(&self) -> String {
       format!("data-{}", self.id) 
    }

    // Subscribe to a topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Error> {
        debug!("Subscribing to resource: {}", topic);

        let mut req_opts = RequestOptions::default();
        req_opts.timeout = Duration::from_secs(10);

        let observer = self.client.observe(topic, &req_opts).await;
        self.subs.push(observer);

        Ok(())
    }

    // Publish data to a topic
    async fn publish(&mut self, topic: &str, data: &[u8]) -> Result<(), Error> {
        let mut req_opts = RequestOptions::default();
        req_opts.timeout = Duration::from_secs(10);

        self.client.put(topic, Some(data), &req_opts).await?;
        Ok(())
    }

    // Close CoAP socket
    async fn close(mut self) -> Result<(), Error> {
        // Remove observations
        for s in self.subs.drain(..) {
            self.client.unobserve(s).await?;
        }

        // Close client socket
        let _ = self.client.close().await;

        Ok(())
    }
}
