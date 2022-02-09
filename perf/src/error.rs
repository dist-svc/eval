

#[derive(Debug)]
pub enum Error {
    Coap(coap_client::Error<std::io::Error>),
    Mqtt(paho_mqtt::errors::Error),
    Dsf(),
    Io(std::io::Error),
    Docker(bollard::errors::Error),
    Json(serde_json::Error),
    Timeout,
    Unknown,
    InvalidTlsConfiguration,
    Address,
}

impl From<paho_mqtt::errors::Error> for Error {
    fn from(e: paho_mqtt::errors::Error) -> Self {
        Self::Mqtt(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<bollard::errors::Error> for Error {
    fn from(e: bollard::errors::Error) -> Self {
        Self::Docker(e)
    }
}


impl From<coap_client::Error<std::io::Error>> for Error {
    fn from(e: coap_client::Error<std::io::Error>) -> Self {
        Self::Coap(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}