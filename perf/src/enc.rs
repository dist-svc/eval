
use dsf_core::{options, prelude::*, base::Encode, types::DateTime};
use dsf_iot::{prelude::*, endpoint::{EpValue, EpKind}};

use serde::{Serialize};
use serde_json::json;

#[derive(Debug, Serialize)]
struct ExampleService {
    endpoints: &'static [EpKind],
    meta: &'static [dsf_core::options::Options],
}

const SERVICE: ExampleService = ExampleService {
    endpoints: &[EpKind::Temperature, EpKind::Pressure, EpKind::Humidity],
    meta: &[dsf_core::options::Options::Coord(options::Coordinates{ lat: 47.874, lng: 88.566, alt: 100.0})]
};

#[derive(Debug, Serialize)]
struct ExampleData {
    data: &'static [ExampleValue]
}

#[derive(Debug, Serialize)]

struct ExampleValue {
    kind: EpKind,
    value: f32,
    unit: &'static str,
}

const DATA: ExampleData = ExampleData {
    data: &[  
        ExampleValue{kind: EpKind::Temperature, value: 27.3, unit: "C"},
        ExampleValue{kind: EpKind::Pressure, value: 1024.0, unit: "kPa"},
        ExampleValue{kind: EpKind::Humidity, value: 49.3, unit: "%RH"},
    ]
};

const ENDPOINTS: [EpDescriptor; 3] = [
    EpDescriptor{kind: EpKind::Temperature, flags: EpFlags::empty() },
    EpDescriptor{kind: EpKind::Pressure, flags: EpFlags::empty() },
    EpDescriptor{kind: EpKind::Humidity, flags: EpFlags::empty() },
];

const OPTIONS: [dsf_core::options::Options; 1] = [
    dsf_core::options::Options::Coord(options::Coordinates{ lat: 47.874, lng: 88.566, alt: 100.0}),
];

const ENDPOINT_DATA: [EpData; 3] = [
    EpData{ value: EpValue::Float32(27.3) },
    EpData{ value: EpValue::Float32(1024.0) },
    EpData{ value: EpValue::Float32(49.3) },
];

#[test]
fn encode_json_service() {
    let s = serde_json::to_string(&SERVICE).unwrap();
    println!("JSON encoded service: {}, {} bytes", s, s.len());
}

#[test]
fn encode_json_data() {
    let s = serde_json::to_string(&DATA).unwrap();
    println!("JSON encoded data: {}, {} bytes", s, s.len());
}


#[test]
fn encode_cbor_service() {
    let b = serde_cbor::to_vec(&SERVICE).unwrap();
    println!("CBOR encoded service: {} bytes", b.len());
}


#[test]
fn encode_cbor_data() {
    let b = serde_cbor::to_vec(&DATA).unwrap();
    println!("CBOR encoded data: {} bytes", b.len());
}

#[test]
fn encode_dsf_service() {
    // Create endpoints

    let mut body = [0u8; 1024];
    let n = Encode::encode(&ENDPOINTS, &mut body).unwrap();
   
    let mut s = ServiceBuilder::default()
        .body(Body::Cleartext((&body[..n]).to_vec()))
        .public_options(OPTIONS.to_vec())
        .encrypt()
        .build().unwrap();
 
    let mut buff = [0u8; 1024];
    let (n, _p) = s.publish_primary(PrimaryOptions{ issued: Some(DateTime::now()), expiry: None }, &mut buff).unwrap();
    let primary = &buff[..n];

    println!("DSF encoded service: {:?}, {} bytes", ENDPOINTS, primary.len());
}

#[test]
fn encode_dsf_data() {

    let mut s = ServiceBuilder::default()
        .body(Body::None)
        .encrypt()
        .build().unwrap();

    let mut data_buff = [0u8; 128];
    let n = Encode::encode(&ENDPOINT_DATA, &mut data_buff).unwrap();

    let page_opts = DataOptions{
        body: Body::Cleartext((&data_buff[..n])),
        ..Default::default()
    };

    let mut page_buff = [0u8; 512];
    let (n, _p) = s.publish_data(page_opts, &mut page_buff[..]).unwrap();
    let data = &page_buff[..n];

    println!("DSF encoded data: {:?}, {} bytes", ENDPOINT_DATA, data.len());
}
