#include "ky_tunnel.h"
#include "ky_protocol.h"
#include "ky_frame.h"
#include "ky_codec.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>
#include <errno.h>
#include <time.h>
#include "ky_tunnel.h"
#include "ky_protocol.h"
#include "ky_frame.h"
#include "ky_codec.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/time.h>       /* for struct timeval */
#include <netinet/in.h>
#include <arpa/inet.h>
#include <netdb.h>          /* for getaddrinfo, struct addrinfo */
#include <errno.h>
#include <time.h>


/* Tunnel configuration */
typedef struct {
    char    host[256];
    int     port;
    int     timeout_ms;
    int     max_conns;
    int     dns_cache;
    /* Additional fields for connection pool (simplified: single persistent connection) */
} ky_tunnel_config_t;

/* Main tunnel structure */
struct KyTunnel {
    ky_tunnel_config_t config;
    int                sockfd;      /* active socket, -1 if none */
    uint32_t           session_id;  /* current session id */
    int                connected;   /* flag */
};

/* ---------- Helper Functions ---------- */

static void ky_tunnel_default_config(ky_tunnel_config_t *cfg) {
    memset(cfg, 0, sizeof(*cfg));
    strcpy(cfg->host, "127.0.0.1");
    cfg->port = 9090;
    cfg->timeout_ms = 5000;
    cfg->max_conns = 10;
    cfg->dns_cache = 1;
}

static int ky_tunnel_parse_config(const char *config_str, ky_tunnel_config_t *cfg) {
    ky_tunnel_default_config(cfg);
    if (!config_str) return 0; /* use defaults */

    char *copy = strdup(config_str);
    if (!copy) return -1;

    char *saveptr = NULL;
    char *token = strtok_r(copy, ";", &saveptr);
    while (token) {
        char key[64] = {0};
        char val[256] = {0};
        if (sscanf(token, "%63[^=]=%255s", key, val) == 2) {
            if (strcmp(key, "host") == 0) {
                strncpy(cfg->host, val, sizeof(cfg->host)-1);
                cfg->host[sizeof(cfg->host)-1] = '\0';
            } else if (strcmp(key, "port") == 0) {
                cfg->port = atoi(val);
            } else if (strcmp(key, "timeout_ms") == 0) {
                cfg->timeout_ms = atoi(val);
            } else if (strcmp(key, "max_conns") == 0) {
                cfg->max_conns = atoi(val);
            } else if (strcmp(key, "dns_cache") == 0) {
                cfg->dns_cache = atoi(val);
            }
        }
        token = strtok_r(NULL, ";", &saveptr);
    }
    free(copy);
    return 0;
}

static int ky_tunnel_connect(KyTunnel *tunnel) {
    if (tunnel->connected) return 0;

    struct addrinfo hints, *res;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    char port_str[16];
    snprintf(port_str, sizeof(port_str), "%d", tunnel->config.port);

    int ret = getaddrinfo(tunnel->config.host, port_str, &hints, &res);
    if (ret != 0) {
        return KY_TUNNEL_ERR_CONNECT;
    }

    int sockfd = -1;
    struct addrinfo *rp;
    for (rp = res; rp != NULL; rp = rp->ai_next) {
        sockfd = socket(rp->ai_family, rp->ai_socktype, rp->ai_protocol);
        if (sockfd == -1) continue;
        if (connect(sockfd, rp->ai_addr, rp->ai_addrlen) == 0) {
            break; /* success */
        }
        close(sockfd);
        sockfd = -1;
    }
    freeaddrinfo(res);

    if (sockfd == -1) {
        return KY_TUNNEL_ERR_CONNECT;
    }

    /* Set receive timeout */
    struct timeval tv;
    tv.tv_sec = tunnel->config.timeout_ms / 1000;
    tv.tv_usec = (tunnel->config.timeout_ms % 1000) * 1000;
    setsockopt(sockfd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(sockfd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));

    tunnel->sockfd = sockfd;
    tunnel->connected = 1;
    return 0;
}

static int ky_tunnel_send_frame(KyTunnel *tunnel, const char *frame, size_t len) {
    if (!tunnel->connected) return KY_TUNNEL_ERR_CONNECT;

    size_t sent = 0;
    while (sent < len) {
        ssize_t n = send(tunnel->sockfd, frame + sent, len - sent, 0);
        if (n <= 0) {
            if (errno == EINTR) continue;
            return KY_TUNNEL_ERR_SEND;
        }
        sent += n;
    }
    return 0;
}

static int ky_tunnel_recv_frame(KyTunnel *tunnel, char **out_frame, size_t *out_len) {
    if (!tunnel->connected) return KY_TUNNEL_ERR_CONNECT;

    /* We first read the fixed header to determine body length */
    char header[KY_RESPONSE_HEADER_SIZE];
    size_t received = 0;
    while (received < KY_RESPONSE_HEADER_SIZE) {
        ssize_t n = recv(tunnel->sockfd, header + received, KY_RESPONSE_HEADER_SIZE - received, 0);
        if (n <= 0) {
            if (errno == EINTR) continue;
            if (n == 0) {
                /* connection closed */
                close(tunnel->sockfd);
                tunnel->sockfd = -1;
                tunnel->connected = 0;
                return KY_TUNNEL_ERR_RECV;
            }
            return KY_TUNNEL_ERR_RECV;
        }
        received += n;
    }

    /* Parse header manually to avoid full-frame validation */
    if (memcmp(header, KY_MAGIC, KY_MAGIC_LEN) != 0) return KY_TUNNEL_ERR_PROTOCOL;

    const unsigned char *hp = (const unsigned char *)header + KY_MAGIC_LEN;
    hp += 2; /* skip status and flags */
    uint32_t body_len = ((uint32_t)hp[0] << 24) |
                        ((uint32_t)hp[1] << 16) |
                        ((uint32_t)hp[2] << 8)  |
                         (uint32_t)hp[3];
    size_t total_len = KY_RESPONSE_HEADER_SIZE + body_len;
    char *frame = (char *)malloc(total_len);
    if (!frame) return KY_TUNNEL_ERR_NOMEM;

    memcpy(frame, header, KY_RESPONSE_HEADER_SIZE);

    /* Read body if any */
    if (body_len > 0) {
        received = 0;
        while (received < body_len) {
            ssize_t n = recv(tunnel->sockfd, frame + KY_RESPONSE_HEADER_SIZE + received,
                             body_len - received, 0);
            if (n <= 0) {
                if (errno == EINTR) continue;
                free(frame);
                if (n == 0) {
                    close(tunnel->sockfd);
                    tunnel->sockfd = -1;
                    tunnel->connected = 0;
                    return KY_TUNNEL_ERR_RECV;
                }
                return KY_TUNNEL_ERR_RECV;
            }
            received += n;
        }
    }

    *out_frame = frame;
    *out_len = total_len;
    return 0;
}

/* ---------- Public API ---------- */

KyTunnel *ky_tunnel_create(const char *config) {
    KyTunnel *tunnel = (KyTunnel *)calloc(1, sizeof(KyTunnel));
    if (!tunnel) return NULL;

    if (ky_tunnel_parse_config(config, &tunnel->config) != 0) {
        free(tunnel);
        return NULL;
    }

    tunnel->sockfd = -1;
    tunnel->connected = 0;
    tunnel->session_id = 1; /* start at 1 */

    if (ky_tunnel_connect(tunnel) != 0) {
        free(tunnel);
        return NULL;
    }

    return tunnel;
}

int ky_tunnel_fetch(KyTunnel *tunnel, const char *url,
                    char **out_body, size_t *out_len) {
    if (!tunnel || !url || !out_body || !out_len) return KY_TUNNEL_ERR_INVALID;
    if (!tunnel->connected) return KY_TUNNEL_ERR_CONNECT;

    /* Build request frame */
    ky_request_header_t req_header;
    ky_protocol_init_request_header(&req_header);
    req_header.cmd = KY_CMD_FETCH;
    req_header.flags = 0; /* could set TLS_SPOOF if needed */
    req_header.session_id = tunnel->session_id;
    req_header.url_len = (uint16_t)strlen(url);


    size_t frame_len = KY_REQUEST_HEADER_SIZE + req_header.url_len;
    char *frame = (char *)malloc(frame_len);
    if (!frame) return KY_TUNNEL_ERR_NOMEM;

    if (ky_frame_build_request(&req_header, url, req_header.url_len, frame, &frame_len) != 0) {
        free(frame);
        return KY_TUNNEL_ERR_PROTOCOL;
    }

    /* Send frame */
    int ret = ky_tunnel_send_frame(tunnel, frame, frame_len);
    free(frame);
    if (ret != 0) return ret;

    /* Receive response */
    char *resp_frame = NULL;
    size_t resp_len = 0;
    ret = ky_tunnel_recv_frame(tunnel, &resp_frame, &resp_len);
    if (ret != 0) return ret;

    /* Parse response */
    ky_response_header_t resp_header;
    const char *body = NULL;
    size_t body_len = 0;
    if (ky_frame_parse_response(resp_frame, resp_len, &resp_header, &body, &body_len) != 0) {
        free(resp_frame);
        return KY_TUNNEL_ERR_PROTOCOL;
    }

    if (resp_header.status != KY_STATUS_OK) {
        free(resp_frame);
        return KY_TUNNEL_ERR_PROTOCOL; /* map status to error */
    }

    /* Allocate and copy body */
    char *out = (char *)malloc(body_len + 1);
    if (!out) {
        free(resp_frame);
        return KY_TUNNEL_ERR_NOMEM;
    }
    if (body_len > 0) {
        memcpy(out, body, body_len);
    }
    out[body_len] = '\0';

    free(resp_frame);
    *out_body = out;
    *out_len = body_len;
    return KY_TUNNEL_OK;
}

int ky_tunnel_batch(KyTunnel *tunnel, const char **urls, int count,
                    KyTunnelResult **results) {
    /* For simplicity, we'll do sequential fetches. A true batch would send all
       URLs in one frame and receive multiple responses. */
    if (!tunnel || !urls || count <= 0 || !results) return KY_TUNNEL_ERR_INVALID;
    if (!tunnel->connected) return KY_TUNNEL_ERR_CONNECT;

    KyTunnelResult *res = (KyTunnelResult *)calloc(count, sizeof(KyTunnelResult));
    if (!res) return KY_TUNNEL_ERR_NOMEM;

    for (int i = 0; i < count; i++) {
        res[i].url = strdup(urls[i]);
        if (!res[i].url) {
            /* free previous */
            for (int j = 0; j < i; j++) {
                free(res[j].url);
                if (res[j].body) free(res[j].body);
            }
            free(res);
            return KY_TUNNEL_ERR_NOMEM;
        }

        int ret = ky_tunnel_fetch(tunnel, urls[i], &res[i].body, &res[i].body_len);
        if (ret == KY_TUNNEL_OK) {
            res[i].status = KY_STATUS_OK;
        } else {
            res[i].status = KY_STATUS_ERROR;
            res[i].body = NULL;
            res[i].body_len = 0;
        }
    }

    *results = res;
    return KY_TUNNEL_OK;
}

int ky_tunnel_ping(KyTunnel *tunnel) {
    if (!tunnel) return KY_TUNNEL_ERR_INVALID;
    if (!tunnel->connected) return KY_TUNNEL_ERR_CONNECT;

    ky_request_header_t req;
    ky_protocol_init_request_header(&req);
    req.cmd = KY_CMD_PING;
    req.flags = 0;
    req.session_id = tunnel->session_id;
    req.url_len = 0;

    size_t frame_len = KY_REQUEST_HEADER_SIZE;
    char frame[KY_REQUEST_HEADER_SIZE];
    if (ky_frame_build_request(&req, NULL, 0, frame, &frame_len) != 0) {
        return KY_TUNNEL_ERR_PROTOCOL;
    }

    int ret = ky_tunnel_send_frame(tunnel, frame, frame_len);
    if (ret != 0) return ret;

    char *resp_frame;
    size_t resp_len;
    ret = ky_tunnel_recv_frame(tunnel, &resp_frame, &resp_len);
    if (ret != 0) return ret;

    ky_response_header_t resp_header;
    if (ky_frame_parse_response(resp_frame, resp_len, &resp_header, &(const char *){0}, &(size_t){0}) != 0) {
        free(resp_frame);
        return KY_TUNNEL_ERR_PROTOCOL;
    }
    free(resp_frame);

    if (resp_header.status == KY_STATUS_OK) return KY_TUNNEL_OK;
    return KY_TUNNEL_ERR_PROTOCOL;
}

void ky_tunnel_free_results(KyTunnelResult *results, int count) {
    if (!results) return;
    for (int i = 0; i < count; i++) {
        free(results[i].url);
        free(results[i].body);
    }
    free(results);
}

void ky_tunnel_destroy(KyTunnel *tunnel) {
    if (!tunnel) return;
    if (tunnel->connected) {
        close(tunnel->sockfd);
        tunnel->sockfd = -1;
        tunnel->connected = 0;
    }
    free(tunnel);
}