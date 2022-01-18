
use futures::stream::Stream;
use async_trait::async_trait;

pub mod driver_coap;
pub use driver_coap::CoapDriver;

pub mod driver_mqtt;
pub use driver_mqtt::MqttDriver;

pub mod driver_dsf;
pub use driver_dsf::DsfDriver;

pub mod driver_loop;
pub use driver_loop::LoopDriver;

use crate::error::Error;

#[derive(Clone, PartialEq, Debug)]
pub struct ClientOpts {
    pub use_tls: bool,

    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

impl Default for ClientOpts {
    fn default() -> Self {
        Self {
            use_tls: false,
            tls_ca: None,
            tls_cert: None,
            tls_key: None,
        }
    }
}

#[async_trait]
pub trait Driver: Clone + Send + Sync {
    type Client;

    /// Create a new client using the provided driver
    async fn new(&self, server: String, id: usize, name: String) -> Result<Self::Client, Error>;
}

#[async_trait]
pub trait Client: Stream<Item = Vec<u8>> {
    fn id(&self) -> usize;

    fn topic(&self) -> String;

    // Subscribe to a topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Error>;

    async fn register(&mut self, _topic: &str) -> Result<(), Error> {
        Ok(())
    }

    // Publish data to a topic
    async fn publish(&mut self, topic: &str, data: &[u8]) -> Result<(), Error>;

    // Disconnect a client
    async fn close(self) -> Result<(), Error>;

}
