
use dsf_core::{options, prelude::*};
use dsf_iot::{prelude::*, endpoint::{EndpointValue, EndpointKind}};

use serde::{Serialize};
use serde_json::json;

#[derive(Debug, Serialize)]
struct ExampleService {
    endpoints: &'static [EndpointKind],
    meta: &'static [dsf_core::options::Options],
}

const SERVICE: ExampleService = ExampleService {
    endpoints: &[EndpointKind::Temperature, EndpointKind::Pressure, EndpointKind::Humidity],
    meta: &[dsf_core::options::Options::Coord(options::Coordinates{ lat: 47.874, lng: 88.566, alt: 100.0})]
};

#[derive(Debug, Serialize)]
struct ExampleData {
    data: &'static [ExampleValue]
}

#[derive(Debug, Serialize)]

struct ExampleValue {
    kind: EndpointKind,
    value: f32,
    unit: &'static str,
}

const DATA: ExampleData = ExampleData {
    data: &[  
        ExampleValue{kind: EndpointKind::Temperature, value: 27.3, unit: "C"},
        ExampleValue{kind: EndpointKind::Pressure, value: 1024.0, unit: "kPa"},
        ExampleValue{kind: EndpointKind::Humidity, value: 49.3, unit: "%RH"},
    ]
};

const ENDPOINTS: [EndpointDescriptor; 3] = [
    EndpointDescriptor{kind: EndpointKind::Temperature, meta: vec![]},
    EndpointDescriptor{kind: EndpointKind::Pressure, meta: vec![]},
    EndpointDescriptor{kind: EndpointKind::Humidity, meta: vec![]},
];

const OPTIONS: [dsf_core::options::Options; 1] = [
    dsf_core::options::Options::Coord(options::Coordinates{ lat: 47.874, lng: 88.566, alt: 100.0}),
];

const ENDPOINT_DATA: [EndpointData; 3] = [
    EndpointData{ value: EndpointValue::Float32(27.3), meta: vec![]},
    EndpointData{ value: EndpointValue::Float32(1024.0), meta: vec![]},
    EndpointData{ value: EndpointValue::Float32(49.3), meta: vec![]},
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
    let n = IotService::encode_body(&ENDPOINTS, &mut body).unwrap();
   
    let mut s = ServiceBuilder::default()
        .body(Body::Cleartext((&body[..n]).to_vec()))
        .public_options(OPTIONS.to_vec())
        .encrypt()
        .build().unwrap();
 
    let mut buff = [0u8; 1024];
    let (n, _p) = s.publish_primary(&mut buff).unwrap();
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
    let n = IotData::encode_data(&ENDPOINT_DATA, &mut data_buff).unwrap();

    let page_opts = DataOptions{
        body: Body::Cleartext((&data_buff[..n]).to_vec()),
        ..Default::default()
    };

    let mut page_buff = [0u8; 512];
    let (n, _p) = s.publish_data(page_opts, &mut page_buff[..]).unwrap();
    let data = &page_buff[..n];

    println!("DSF encoded data: {:?}, {} bytes", ENDPOINT_DATA, data.len());
}
