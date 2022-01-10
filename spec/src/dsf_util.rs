
use core::convert::TryFrom;

use dsf_core::prelude::{Body, DataOptions, Page};
use dsf_core::service::{Service, ServiceBuilder, Publisher};

use dsf_iot::prelude::*;
use dsf_iot::endpoint::Value;

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {

    for s in SERVICES {
        println!("Generating '{}' service (encrypted: {:?})", s.0, s.3);

        let (page, data) = build_service(s.1, s.2, s.3)?;

        println!("Page: {} bytes, Data: {} bytes", page, data);
    }

    Ok(())
}

const SERVICES: &[(&str, &[Descriptor], &[Data], bool)] = &[
    ("env", ENV_DESCRIPTORS, ENV_DATA, false),
    ("env", ENV_DESCRIPTORS, ENV_DATA, true),
    ("light", LIGHT_DESCRIPTORS, LIGHT_DATA, false),
    ("light", LIGHT_DESCRIPTORS, LIGHT_DATA, true),
];

const ENV_DESCRIPTORS: &[Descriptor] = &[
    Descriptor{kind: Kind::Temperature, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Humidity, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Pressure, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Co2, flags: Flags::R, meta: vec![]},
];

const ENV_DATA: &[Data] = &[
    Data{ value: Value::Float32(27.3), meta: vec![] },
    Data{ value: Value::Float32(49.5), meta: vec![] },
    Data{ value: Value::Float32(100.4), meta: vec![] },
    Data{ value: Value::Int32(233), meta: vec![] },
];

const LIGHT_DESCRIPTORS: &[Descriptor] = &[
    Descriptor{kind: Kind::State, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Brightness, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Colour, flags: Flags::R, meta: vec![]},
];

const LIGHT_DATA: &[Data] = &[

];


fn build_service(desc: &[Descriptor], data: &[Data], encrypted: bool) -> Result<(usize, usize), anyhow::Error> {

    let mut ep_buff = [0u8; 1024];
    let n = IotService::encode_body(desc, &mut ep_buff).unwrap();

    // TODO: set kind to IotService
    let mut sb = ServiceBuilder::default()
        .body(Body::Cleartext((&ep_buff[..n]).to_vec()));

    if encrypted {
        sb = sb.encrypt();
    }

    let mut s = sb.build()?;

    // Build primary page

    let mut buff = [0u8; 1024];
    let (page_len, page) = s.publish_primary(Default::default(), &mut buff)?;

    let p = Page::try_from(page)?;
    log::debug!("Page: {:?}", p);

    // Build data
    let mut data_buff = [0u8; 1024];
    let n = IotData::encode_data(data, &mut data_buff)?;

    let data_opts = DataOptions{
        body: &data_buff[..n],
        ..Default::default()
    };
    let (data_len, data) = s.publish_data(data_opts, buff)?;

    let p = Page::try_from(data)?;
    log::debug!("Block: {:?}", p);

    Ok((page_len, data_len))
}