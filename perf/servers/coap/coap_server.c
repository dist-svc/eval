
/* minimal CoAP server
 *
 * Copyright (C) 2018 Olaf Bergmann <bergmann@tzi.org>
 * https://github.com/obgm/libcoap-minimal
 */

#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <stdbool.h>
#include <unistd.h>
#include <signal.h>
#include <pthread.h>
#include <errno.h>
#include <sys/types.h>
#include <sys/socket.h>
#include <netdb.h>
#include <getopt.h>

#include <coap3/coap.h>

//#define COAP_USE_MUTEX
#define COAP_DTLS

static volatile int running = 1;

void interrupt_handler(int code) {
    running = 0;
}

typedef struct {
    uint8_t* data;
    size_t data_len;
    pthread_mutex_t mutex;
} resource_ctx_s;


void get_handle(coap_resource_t* resource, 
            coap_session_t* sess, const coap_pdu_t* req, 
            const coap_string_t* query, coap_pdu_t* resp) {

    coap_context_t* coap_ctx = coap_session_get_context(sess);

    resource_ctx_s* resource_ctx = coap_resource_get_userdata(resource);

#ifdef COAP_USE_MUTEX
    pthread_mutex_lock(&resource_ctx->mutex);
#endif

    // Write data if we have some
    if(resource_ctx->data) {
        coap_add_data(resp, resource_ctx->data_len, resource_ctx->data);
    }

    // Respond with OK
    coap_pdu_set_code(resp, COAP_RESPONSE_CODE(205));
#ifdef COAP_USE_MUTEX
    pthread_mutex_unlock(&resource_ctx->mutex);
#endif
}

void put_handle(struct coap_resource_t* resource, 
            coap_session_t* sess, const coap_pdu_t* req, 
            const coap_string_t* query, coap_pdu_t* resp) {

    coap_context_t* coap_ctx = coap_session_get_context(sess);
    resource_ctx_s* resource_ctx = coap_resource_get_userdata(resource);

    const uint8_t* data;
    size_t size = 0;
    
    // Fetch data from request
    coap_get_data(req, &size, &data);
    if (size == 0) {
        return;
    }

#ifdef COAP_USE_MUTEX
    pthread_mutex_lock(&resource_ctx->mutex);
#endif

    // Free existing data
    if(resource_ctx->data) {
        free(resource_ctx->data);
    }

    // Create new data object
    resource_ctx->data = malloc(size);
    memcpy(resource_ctx->data, data, size);
    resource_ctx->data_len = size;

#ifdef COAP_USE_MUTEX   
    pthread_mutex_unlock(&resource_ctx->mutex);
#endif

    // Notify observers
    coap_resource_notify_observers(resource, NULL);

    // Return OK response
    coap_pdu_set_code(resp, COAP_RESPONSE_CODE(201));
}


int resolve_address(const char* hostname, const char* port, coap_address_t* coap_addr) {

  struct addrinfo *res, *ainfo;
  struct addrinfo hints;
  int error, len=-1;

  memset(&hints, 0, sizeof(hints));
  memset(coap_addr, 0, sizeof(*coap_addr));
  hints.ai_socktype = SOCK_DGRAM;
  hints.ai_family = AF_UNSPEC;

  error = getaddrinfo(hostname, port, &hints, &res);

  if (error != 0) {
    fprintf(stderr, "getaddrinfo: %s\n", gai_strerror(error));
    return error;
  }

  for (ainfo = res; ainfo != NULL; ainfo = ainfo->ai_next) {
    switch (ainfo->ai_family) {
    case AF_INET6:
    case AF_INET:
      len = coap_addr->size = ainfo->ai_addrlen;
      memcpy(&coap_addr->addr.sin6, ainfo->ai_addr, coap_addr->size);
      goto finish;
    default:
      ;
    }
  }

 finish:

  freeaddrinfo(res);
  return len;
}


static struct option long_options[] = {
    {"endpoints", required_argument, 0,  0 },
    {"dtls-ca",   required_argument, 0,  0 },
    {"dtls-cert", required_argument, 0,  0 },
    {"dtls-key",  required_argument, 0,  0 },
    {"hostname",  required_argument, 0,  0 },
    {"port",      required_argument, 0,  0 },
    {"debug",     no_argument,       0,  0 },
    {"dtls",     no_argument,       0,  0 },
    {0,           0,                 0,  0 }
};

char* ca_file = ".certs/ca.crt";
char* cert_file = ".certs/client.crt";
char* key_file = ".certs/client.key";
int endpoints = 1;

char* hostname = "0.0.0.0";
char* port = "5683";
bool debug = false;
bool use_dtls = false;

char* read_file(char* path) {
    FILE *fp = fopen(path, "r");
    if (!fp) {
        printf("Error opening file '%s': %s", path, strerror(errno));
        return NULL;
    }

    // Find file size
    fseek(fp, 0L, SEEK_END);
    int size = ftell(fp);
    fseek(fp, 0L, SEEK_SET);

    // Allocate data
    char* data = malloc(size);

    // Read file
    int read = fread(data, 1, size, fp);
    if (size != read) {
        printf("Error reading file '%s': %s", path, strerror(errno));
        return NULL;
    }

    return data;
}

int main(int argc, char **argv) {
    int res = 0;

    // Parse out arguments
    while (1) {
        int option_index = 0;

        int c = getopt_long(argc, argv, "",
                 long_options, &option_index);
        if (c == -1)
            break;

        if (c != 0) {
            printf("getopt returned character code 0%o (arg: %s)\n", c, optarg);
            continue;
        }

        switch (option_index) {
        case 0:
            endpoints = atoi(optarg);
            break;
        case 1:
            ca_file = optarg;
            break;
        case 2:
            cert_file = optarg;
            break;
        case 3:
            key_file = optarg;
            break;
        case 4:
            hostname = optarg;
            break;
        case 5:
            port = optarg;
            break;
        case 6:
            debug = true;
            break;
        case 7:
            use_dtls = true;
            break;
        default:
            printf("Invalid argument index: %d\r\n", c);
        }
    }

    // Setup CoAP context
    coap_context_t *ctx = NULL;
    coap_resource_t *resource = NULL;
    coap_endpoint_t *endpoint = NULL;

    int result = EXIT_FAILURE;


    // Setup bind address
    coap_address_t dst;
    resolve_address(hostname, port, &dst);

    // Initialise CoAP
    coap_startup();

    if (debug) {
        coap_set_log_level(LOG_DEBUG);
    }

    /* create CoAP context and a client session */
    ctx = coap_new_context(&dst);

    uint32_t role = COAP_DTLS_ROLE_SERVER;
    coap_dtls_pki_t coap_pki = {
        .version = COAP_DTLS_PKI_SETUP_VERSION,
        .verify_peer_cert        = 0,
        //.require_peer_cert       = 0,
        .allow_self_signed       = 1,
        .allow_expired_certs     = 1,
        .cert_chain_validation   = 1,
        .cert_chain_verify_depth = 2,
        .check_cert_revocation   = 0,
        .allow_no_crl            = 1,
        .allow_expired_crl       = 1,
    };

    coap_proto_t coap_proto = COAP_PROTO_UDP;

    if (use_dtls && ca_file && cert_file && key_file) {
        printf("Using DTLS CA: %s cert: %s key: %s\r\n",
                ca_file, cert_file, key_file);

        // Load keys

        coap_pki.pki_key.key_type = COAP_PKI_KEY_PEM;
        coap_pki.pki_key.key.pem.public_cert = cert_file;
        coap_pki.pki_key.key.pem.private_key = key_file;
        coap_pki.pki_key.key.pem.ca_file = ca_file;

        res = coap_context_set_pki_root_cas(ctx, ca_file, NULL);
        if (res < 0) {
            printf("COAP DTLS set root CA: %d", res);
            goto finish;
        }

        res = coap_context_set_pki(ctx, &coap_pki);
        if (res < 0) {
            printf("COAP DTLS setup error: %d", res);
            goto finish;
        }

        if (debug) {
            coap_dtls_set_log_level(LOG_DEBUG);
        }
        coap_proto = COAP_PROTO_DTLS;
    }

    // Setup endpoint
    if (!ctx || !(endpoint = coap_new_endpoint(ctx, &dst, coap_proto))) {
        coap_log(LOG_EMERG, "cannot initialize context\n");
        goto finish;
    }

    // Register resources
    coap_resource_t **resources = (coap_resource_t **) calloc(sizeof(void*), endpoints);

    for (int i=0; i<endpoints; i++) {

      // Setup resource context
      resource_ctx_s *resource_ctx = (resource_ctx_s*) calloc(sizeof(resource_ctx_s), 1);
#ifdef COAP_USE_MUTEX
      pthread_mutex_init(&resource_ctx.mutex, NULL);
#endif
      // Register resource
      char endpoint_name[32] = { 0 };
      snprintf(endpoint_name, sizeof(endpoint_name), "data-%d", i);
      
      coap_str_const_t *path = coap_new_str_const(endpoint_name, strlen(endpoint_name));
      //COAP_SET_STR(path, endpoint_name, strlen(endpoint_name)-1);
      resources[i] = coap_resource_init(path, 0);

    if(debug) {
        printf("Registering endpoint: %s\r\n", endpoint_name);
    }

      // Register handlers
      coap_register_handler(resources[i], COAP_REQUEST_GET, &get_handle);
      coap_register_handler(resources[i], COAP_REQUEST_PUT, &put_handle);

      // Bind context and set observable
      coap_resource_set_userdata(resources[i], resource_ctx);
      coap_resource_set_get_observable(resources[i], 1);
      coap_add_resource(ctx, resources[i]);
    }

    // Bind exit handler
    signal(SIGINT, interrupt_handler);

    printf("Running CoAP server at '%s:%s' with %d endpoints\r\n", hostname, port, endpoints);

    // Run the coap server
    while (running) {
        //coap_run_once(ctx, 1000);
        coap_io_process(ctx, 1000);
    }

    printf("Exiting\r\n");

    result = EXIT_SUCCESS;

finish:

    // TODO: free resources etc.

    coap_free_context(ctx);
    coap_cleanup();

    return result;
}

