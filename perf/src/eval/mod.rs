
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::marker::PhantomData;

use log::{trace, debug, error};
use serde::{Serialize, Deserialize};

use futures::stream::StreamExt;
use tokio::sync::broadcast::Receiver;
use tokio::task::{self, JoinHandle};

use rolling_stats::Stats;
use byteorder::{LE, ByteOrder as _};

use crate::error::Error;
use crate::drivers::Client;


#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct TestMessage {
    session: usize,
    //index: usize,
    sent_time_us: u128,
}

#[derive(Debug, PartialEq, Clone)]
pub struct AgentStats  {

}

pub struct Agent<C: Client> {
    handle: JoinHandle<Result<Stats<f64>, Error>>,

    _client: PhantomData<C>,
}

fn now_us() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as u128
}

impl <C> Agent <C> 
where 
    C: Client + Send + Unpin + 'static,
{
    pub async fn new_publisher(mut c: C, mut start: Receiver<()>, mut stop: Receiver<()>, session: usize, topic: String, message_size: usize, period: Duration) -> Result<Self, Error> {

        c.register(&topic).await?;

        let mut stats = Stats::new();
        //let mut index = 0;

        let handle = task::spawn(async move {

            // Configure interval timer
            let mut interval = tokio::time::interval(period);

            // Await start signal
            if let Err(e) = start.recv().await {
                error!("Start RX error: {:?}", e);
                return Err(Error::Unknown)
            }

            loop {
                // Handle intervals and exit signals
                tokio::select! {
                    _ = interval.tick() => {
                        // Build outgoing message
                        let msg = TestMessage {
                            session,
                            //index,
                            sent_time_us: now_us(),
                        };
                        let mut buff = vec![0u8; message_size];
                        
                        let len = bincode::serialized_size(&msg).unwrap();
                        LE::write_u32(&mut buff[0..4], len as u32);

                        bincode::serialize_into(&mut buff[4..], &msg).unwrap();
                        

                        debug!("Publishing data {:?}", msg);
                        trace!("Encoded: {:?}", buff);

                        let i = Instant::now();

                        if let Err(e) = c.publish(&topic, &buff).await {
                            error!("Publish error: {:?}", e);
                        } else {
                            let d = Instant::now().duration_since(i);

                            stats.update(d.as_micros() as f64);
                            //index += 1;
                        }
                    },
                    _exit = stop.recv() => {
                        debug!("Publisher exit");
                        break
                    },
                }
            }

            // Disconnect from the client
            c.close().await?;

            Ok(stats)
        });

        Ok(Self{handle, _client: PhantomData})
    }

    pub async fn new_subscriber(mut c: C, mut start: Receiver<()>, mut stop: Receiver<()>, session: usize, topics: Vec<String>) -> Result<Self, Error> {

        let mut stats = Stats::new();
        
        // Subscribe to provided topic(s)
        for t in &topics {
            c.subscribe(t).await?;
        }
        
        // Start receiver thread
        let handle = task::spawn(async move {     

            // Await start
            if let Err(e) = start.recv().await {
                error!("Start RX error: {:?}", e);
                return Err(Error::Unknown)
            }

            loop {
                // Receive messages and handle the exit signal
                tokio::select! {
                    v = c.next() => {
                        if let Some(v) = v {
                            debug!("Received data: {:?}", v);

                            if v.len() < 4 {
                                continue;
                            }

                            let len = LE::read_u32(&v[0..4]) as usize;
                            let msg: TestMessage = match bincode::deserialize(&v[4..4+len]) {
                                Ok(m) => m,
                                Err(e) => {
                                    error!("bincode decoding error: {:?}", e);
                                    continue;
                                }
                            };

                            if msg.session != session {
                                continue;
                            }

                            let n = now_us();
                            let delay = n - msg.sent_time_us;

                            debug!("Received {:?} at {} us ({} us delay)", msg, n, delay);

                            stats.update(delay as f64);
                        } else {
                            // TODO: should this ever happen? probs not?
                            trace!("Subscriber received None");
                        }
                    },
                    _exit = stop.recv() => {
                        debug!("Subscriber exit");
                        break;
                    },
                }
            }

            debug!("Exiting subscriber handler");

            // Disconnect the client
            c.close().await?;

            Ok(stats)
        });

        Ok(Self{handle, _client: PhantomData})
    }

    pub async fn done(self) -> Result<Stats<f64>, Error> {
        // Await join result
        self.handle.await.unwrap()
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_message_enc_dec() {

        let t = TestMessage{session: 1, sent_time_us: 1599479706319454};
        
        let enc = bincode::serialize(&t).unwrap();

        println!("Encoded: {:?}", enc);

        let dec = bincode::deserialize(&enc).unwrap();
        
        println!("Decoded: {:?}", dec);

        assert_eq!(t, dec);
    }
}