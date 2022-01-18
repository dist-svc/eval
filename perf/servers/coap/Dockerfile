FROM debian:latest

# Fetch dependencies
RUN apt-get update && apt-get install -y \
    build-essential git autoconf automake pkg-config libtool \
    libssl-dev

WORKDIR /root

# Setup libcoap build
RUN git clone https://github.com/obgm/libcoap.git && \
    cd libcoap && \
    ./autogen.sh && \
    ./configure --with-openssl --enable-examples --disable-doxygen --disable-manpages

# Patch for CA validation
#COPY tls-skip-verify.patch /tls-skip-verify.patch
#RUN git -C libcoap apply /tls-skip-verify.patch

# Build libcoap
RUN cd libcoap && make -j && make install

# Build CoAP server
COPY coap_server.c /root
RUN gcc coap_server.c -O2 -o coap-server -lcoap-3-openssl -lpthread  

ENV LD_LIBRARY_PATH=/usr/local/lib

CMD [ "/root/coap-server", "--endpoints=100" ]
