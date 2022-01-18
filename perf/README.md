# IoT Performance Evaluation

Tooling to evaluate the performance of comparable IoT brokers.

## Setup

The test target runs docker to allow management of services and provide a mechanism for the collection of statistics on these services.

## Servers

For (D)TLS modes the docker target must contain viable certificates (`ca.crt`, `client.crt`, `client.key`) at `/etc/certs`.
### DSF

### CoAP

DTLS requires libcoap compiled with `./autogen.sh && ./configure --with-openssl --enable-examples --disable-doxygen && make -j` and mods to `examples/client.c` to allow missing CRL, skip verifying name. These are applied in the dockerfile.

Arguments:

- `--endpoints=N` to set number of endpoints
- `--debug` to enable debug logging
- `--dtls` to enable dtls
- `--hostname=H` to set hostname
- `--port=P` to set port

Can be tested with (modified) libcoap client, via:
```
./examples/coap-client -C ca.crt -c client.crt -j client.key -v 7 coaps://127.0.0.1/data-0 -m get

./examples/coap-client -C ca.crt -c client.crt -j client.key -v 7 coaps://127.0.0.1/data-0 -m put -e "test"
```
### MQTT


