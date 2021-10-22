# Comparing IoT Specifications

## Resources:
- [OCF DeviceBuilder]()

## Process

- Defined environment sensor and light devices in OCF form
- Generated flask app via:
  - `./DeviceBuilder_FlaskServer.sh  ../../phd-eval-spec/specs/ocf-light-def.json ./out oic.d.lightingcontrol`
  - `./DeviceBuilder_FlaskServer.sh  ../../phd-eval-spec/specs/ocf-env-def.json ./out oic.d.airqualitymonitor`
- Fetch JSON objects from `http://localhost:8888/oic/res`
- Convert to CBOR with:
  - `cargo run -- to-cbor ./specs/ocf-env-res.json ./specs/ocf-env-res.cbor`
  - `cargo run -- to-cbor ./specs/ocf-light-res.json ./specs/ocf-light-res.cbor`

