#ifndef KY_TUNNEL_H
#define KY_TUNNEL_H

#include <stddef.h>
#include <stdint.h>

/* Forward declaration */
typedef struct KyTunnel KyTunnel;

/* Result structure for batch operations */
typedef struct {
    char    *url;        /* Original URL requested */
    uint8_t  status;     /* KY_STATUS_* */
    char    *body;       /* Response body (or NULL) */
    size_t   body_len;   /* Length of body */
    uint32_t session_id; /* Session ID used */
    /* Internal fields for memory management */
    void    *_internal;
} KyTunnelResult;

/* Error codes for tunnel operations */
#define KY_TUNNEL_OK            0
#define KY_TUNNEL_ERR_CONNECT  -1
#define KY_TUNNEL_ERR_SEND     -2
#define KY_TUNNEL_ERR_RECV     -3
#define KY_TUNNEL_ERR_PROTOCOL -4
#define KY_TUNNEL_ERR_NOMEM    -5
#define KY_TUNNEL_ERR_INVALID  -6
#define KY_TUNNEL_ERR_TIMEOUT  -7

/*
 * Create a tunnel instance from a configuration string.
 *
 * Configuration format (key=value pairs separated by semicolons):
 *   "host=127.0.0.1;port=9090;timeout_ms=5000;max_conns=10;dns_cache=1"
 *
 * @param config  Configuration string, may be NULL for defaults.
 * @return        New tunnel instance, or NULL on error.
 */
KyTunnel *ky_tunnel_create(const char *config);

/*
 * Fetch a single URL through the tunnel.
 *
 * @param tunnel    Tunnel instance.
 * @param url       URL to fetch (null-terminated string).
 * @param out_body  On success, pointer to allocated body (caller frees with free()).
 * @param out_len   On success, length of body in bytes.
 * @return          KY_TUNNEL_OK on success, or error code.
 */
int ky_tunnel_fetch(KyTunnel *tunnel, const char *url,
                    char **out_body, size_t *out_len);

/*
 * Fetch multiple URLs in a single batch.
 *
 * @param tunnel    Tunnel instance.
 * @param urls      Array of null-terminated URL strings.
 * @param count     Number of URLs in array.
 * @param results   On success, pointer to array of KyTunnelResult (caller frees with ky_tunnel_free_results()).
 *                  Number of results equals count.
 * @return          KY_TUNNEL_OK on success, or error code.
 */
int ky_tunnel_batch(KyTunnel *tunnel, const char **urls, int count,
                    KyTunnelResult **results);

/*
 * Send a ping to the tunnel peer and wait for response.
 *
 * @param tunnel    Tunnel instance.
 * @return          KY_TUNNEL_OK if pong received, error code otherwise.
 */
int ky_tunnel_ping(KyTunnel *tunnel);

/*
 * Destroy a tunnel instance and free all resources.
 * Also releases any pending connections.
 */
void ky_tunnel_destroy(KyTunnel *tunnel);

/*
 * Free the results array returned by ky_tunnel_batch().
 * Frees all internal allocations and the array itself.
 */
void ky_tunnel_free_results(KyTunnelResult *results, int count);

#endif /* KY_TUNNEL_H */