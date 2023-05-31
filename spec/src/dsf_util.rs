
use core::convert::TryFrom;

use dsf_core::base::Encode;
use dsf_core::prelude::{Body, DataOptions};
use dsf_core::service::{Service, ServiceBuilder, Publisher};

use dsf_core::wire::Container;
use dsf_iot::prelude::*;
use dsf_iot::{
    endpoint::{EpKind, EpValue, EpData, EpDescriptor, EpFlags},
};

#[async_std::main]
async fn main() -> Result<(), anyhow::Error> {

    let env_descriptors: &[EpDescriptor] = &[
        EpDescriptor{kind: EpKind::Temperature, flags: EpFlags::R},
        EpDescriptor{kind: EpKind::Humidity, flags: EpFlags::R},
        EpDescriptor{kind: EpKind::Pressure, flags: EpFlags::R},
        EpDescriptor{kind: EpKind::Co2, flags: EpFlags::R},
    ];
    
    let env_data: &[EpData] = &[
        EpData{ value: EpValue::Float32(27.3) },
        EpData{ value: EpValue::Float32(49.5) },
        EpData{ value: EpValue::Float32(100.4) },
        EpData{ value: EpValue::Int32(233) },
    ];
    
    let light_descriptors: &[EpDescriptor] = &[
        EpDescriptor{kind: EpKind::State, flags: EpFlags::R},
        EpDescriptor{kind: EpKind::Brightness, flags: EpFlags::R},
        EpDescriptor{kind: EpKind::Colour, flags: EpFlags::R},
    ];
    
    let light_data: &[EpData] = &[
        EpData{ value: EpValue::Bool(true) },
        EpData{ value: EpValue::Int32(50) },
        EpData{ value: EpValue::try_from(&[255, 255, 255]).unwrap() },
    ];
    

    let services: &[(&str, &[EpDescriptor], &[EpData], bool)] = &[
        ("env", env_descriptors, env_data, false),
        ("env", env_descriptors, env_data, true),
        ("light", light_descriptors, light_data, false),
        ("light", light_descriptors, light_data, true),
    ];


    for s in services {
        println!("Generating '{}' service (encrypted: {:?})", s.0, s.3);

        let (page, data) = build_service(s.1, s.2, s.3)?;

        println!("Page: {} bytes, EpData: {} bytes", page, data);
    }

    Ok(())
}


fn build_service(desc: &[EpDescriptor], data: &[EpData], encrypted: bool) -> Result<(usize, usize), anyhow::Error> {

    let mut ep_buff = [0u8; 1024];
    let svc_info = IotInfo::<8>::new(desc).unwrap();
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

    let p = Container::try_from(page)?;
    log::debug!("Page: {:?}", p);

    // Build data
    let mut data_buff = [0u8; 1024];
    let svc_data = IotData::<8>::new(data).unwrap();
    let n = svc_data.encode(&mut data_buff)?;

    let data_opts = DataOptions{
        body: Some(&data_buff[..n]),
        ..Default::default()
    };
    let (data_len, data) = s.publish_data(data_opts, buff)?;

    let b = Container::try_from(data)?;
    log::debug!("Block: {:?}", b);

    Ok((page_len, data_len))
}