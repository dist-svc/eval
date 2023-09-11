use std::pin::Pin;
use std::task::{Context, Poll};
use std::convert::TryFrom;

use log::debug;
use futures::{Stream, StreamExt};

use tokio::sync::mpsc::{Receiver, channel};
use tokio::task::JoinHandle;

use async_trait::async_trait;

use crate::error::Error;
use super::{Driver, Client, ClientOpts};

#[derive(Clone, PartialEq, Debug)]
pub struct MqttDriver {
    opts: ClientOpts,
}

impl MqttDriver {
    pub fn new(opts: &ClientOpts) -> Self {
        Self{
            opts: opts.clone(),
        }
    }
}

impl TryFrom<&ClientOpts> for paho_mqtt::ConnectOptions {
    type Error = Error;

    fn try_from(o: &ClientOpts) -> Result<paho_mqtt::ConnectOptions, Self::Error> {
        let mut conn_opts = paho_mqtt::ConnectOptionsBuilder::default();

        if o.use_tls {
            debug!("Configuring TLS");
            let mut tls_opts = paho_mqtt::SslOptionsBuilder::new();
            //tls_opts.ssl_version(paho_mqtt::SslVersion::Tls_1_2);
            tls_opts.verify(false);

            // Set TLS CA file if provided
            if let Some(ca_file) = &o.tls_ca {
                tls_opts.trust_store(ca_file.to_string())?;
            }

            // Set TLS certificate / key files if provided
            match (&o.tls_cert, &o.tls_key) {
                (Some(cert_file), Some(key_file)) => {
                    tls_opts.key_store(cert_file.to_string())?;
                    tls_opts.private_key(key_file.to_string())?;
                },
                (Some(_), None) | (None, Some(_)) => {
                    return Err(Error::InvalidTlsConfiguration)
                },
                _ => (),
            }

            conn_opts.ssl_options(tls_opts.finalize());
        }

        Ok(conn_opts.finalize())
    }

}

#[async_trait]
impl Driver for MqttDriver {
    type Client = MqttClient;

    /// Create a new client using the provided driver
    async fn new(&self, server: String, id: usize, name: String) -> Result<Self::Client, Error> {
        // Create client with URI and ID
        let client_opts = paho_mqtt::CreateOptionsBuilder::new()
            .server_uri(server)
            .client_id(name.as_str())
            .persistence(paho_mqtt::PersistenceType::None)
            .finalize();
        let mut client = paho_mqtt::AsyncClient::new(client_opts)?;

        // Build connection options
        let connect_options = paho_mqtt::ConnectOptions::try_from(&self.opts)?;

        // Connect client
        let _resp = client.connect(connect_options).await?;
        
        // Build incoming stream
        let mut mqtt_rx = client.get_stream(1000);
        let (tx, rx) = channel(1000);

        // Spawn adaptor to map from opaque to concrete channel types
        let _handle = tokio::spawn(async move {
            loop {
                // Poll for new messages
                let m = match mqtt_rx.next().await {
                    Some(Some(v)) => v,
                    None => break,
                    _ => continue,
                };

                // Forward to tokio channel
                if let Err(_e) = tx.send( m ).await {
                    break;
                }
            }
        });

        Ok(MqttClient{id, client, rx, _handle})
    }
}


pub struct MqttClient {
    pub id: usize,
    client: paho_mqtt::AsyncClient,
    rx: Receiver<paho_mqtt::Message>,
    _handle: JoinHandle<()>,
}

impl std::fmt::Debug for MqttClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MqttClient").field("id", &self.id).finish()
    }
}

unsafe impl Send for MqttClient {}

impl Stream  for MqttClient {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(m)) =>Poll::Ready(Some( m.payload().to_vec() )),
            Poll::Ready(None) => Poll::Ready(None),
            _ => Poll::Pending,
        }
    }
}

#[async_trait]
impl Client for MqttClient {

    fn id(&self) -> usize {
        self.id
    }

    fn topic(&self) -> String {
        format!("data-{}", self.id) 
    }

    // Subscribe to a topic
    async fn subscribe(&mut self, topic: &str) -> Result<(), Error> {
        self.client.subscribe(topic, 2).await?;
        Ok(())
    }

    // Publish data to a topic
    async fn publish(&mut self, topic: &str, data: &[u8]) -> Result<(), Error> {
        let m = paho_mqtt::Message::new(topic, data, 2);
        self.client.publish(m).await?;
        Ok(())
    }

    async fn close(mut self) -> Result<(), Error> {
        self.client.disconnect(None).await?;
        Ok(())
    }
}
