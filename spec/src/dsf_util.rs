
use core::convert::TryFrom;

use dsf_core::prelude::{Body, DataOptions, Page, Encode};
use dsf_core::service::{Service, ServiceBuilder, Publisher};

use dsf_iot::prelude::*;
use dsf_iot::endpoint::{Value, DataRef};
use dsf_iot::service::IotInfo;

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {

    for s in SERVICES {
        println!("Generating '{}' service (encrypted: {:?})", s.0, s.3);

        let (page, data) = build_service(s.1, s.2, s.3)?;

        println!("Page: {} bytes, Data: {} bytes", page, data);
    }

    Ok(())
}

const SERVICES: &[(&str, &[Descriptor], &[DataRef], bool)] = &[
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

const ENV_DATA: &[DataRef] = &[
    Data{ value: Value::Float32(27.3), meta: &[] },
    Data{ value: Value::Float32(49.5), meta: &[] },
    Data{ value: Value::Float32(100.4), meta: &[] },
    Data{ value: Value::Int32(233), meta: &[] },
];

const LIGHT_DESCRIPTORS: &[Descriptor] = &[
    Descriptor{kind: Kind::State, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Brightness, flags: Flags::R, meta: vec![]},
    Descriptor{kind: Kind::Colour, flags: Flags::R, meta: vec![]},
];

const LIGHT_DATA: &[DataRef] = &[
    Data{ value: Value::Bool(true), meta: &[] },
    Data{ value: Value::Int32(50), meta: &[] },
    Data{ value: Value::Bytes(&[255, 255, 255]), meta: &[] },
];


fn build_service(desc: &[Descriptor], data: &[DataRef], encrypted: bool) -> Result<(usize, usize), anyhow::Error> {

    let mut ep_buff = [0u8; 1024];
    let svc_info = IotInfo::new(desc);
    let n = svc_info.encode(&mut ep_buff).unwrap();

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
    let svc_data = IotData::new(data);
    let n = svc_data.encode(&mut data_buff)?;

    let data_opts = DataOptions{
        body: Some(&data_buff[..n]),
        ..Default::default()
    };
    let (data_len, data) = s.publish_data(data_opts, buff)?;

    let b = Page::try_from(data)?;
    log::debug!("Block: {:?}", b);

    Ok((page_len, data_len))
}