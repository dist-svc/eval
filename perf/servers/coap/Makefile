
COAP_LIB_DIR=x86_64

all: coap-server

coap-server: coap_server.c
	gcc $< -O2 -g -o $@ -I/usr/local/include -L/usr/local/lib -lcoap-2-openssl -lpthread

clean:
	rm coap-server

.PHONY: clean coap-server
