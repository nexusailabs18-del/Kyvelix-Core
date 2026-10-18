package kytunnel

/*
#cgo CFLAGS: -I../include
#cgo LDFLAGS: -L../build -lky_tunnel
#include <stdlib.h>
#include "ky_tunnel.h"
*/
import "C"
import (
    "errors"
    "unsafe"
)

// Tunnel represents a KY Tunnel connection
type Tunnel struct {
    handle *C.KyTunnel
    session uint32
}

// NewTunnel creates a new tunnel connection
func NewTunnel(addr string) (*Tunnel, error) {
    config := C.CString("host=" + addr + ";port=9090;timeout_ms=5000")
    defer C.free(unsafe.Pointer(config))
    
    handle := C.ky_tunnel_create(config)
    if handle == nil {
        return nil, errors.New("failed to create tunnel")
    }
    
    return &Tunnel{
        handle: handle,
        session: 1,
    }, nil
}

// Fetch fetches a single URL through the tunnel
func (t *Tunnel) Fetch(url string) ([]byte, error) {
    cURL := C.CString(url)
    defer C.free(unsafe.Pointer(cURL))
    
    var body *C.char
    var bodyLen C.size_t
    
    ret := C.ky_tunnel_fetch(t.handle, cURL, &body, &bodyLen)
    if ret != C.KY_TUNNEL_OK {
        return nil, errors.New("fetch failed")
    }
    defer C.free(unsafe.Pointer(body))
    
    return C.GoBytes(unsafe.Pointer(body), C.int(bodyLen)), nil
}

// Batch fetches multiple URLs
func (t *Tunnel) Batch(urls []string) ([][]byte, error) {
    // Convert Go strings to C array
    cURLs := make([]*C.char, len(urls))
    for i, url := range urls {
        cURLs[i] = C.CString(url)
        defer C.free(unsafe.Pointer(cURLs[i]))
    }
    
    var results *C.KyTunnelResult
    ret := C.ky_tunnel_batch(t.handle, &cURLs[0], C.int(len(urls)), &results)
    if ret != C.KY_TUNNEL_OK {
        return nil, errors.New("batch fetch failed")
    }
    defer C.ky_tunnel_free_results(results, C.int(len(urls)))
    
    // Convert results back to Go
    output := make([][]byte, len(urls))
    for i := 0; i < len(urls); i++ {
        result := (*C.KyTunnelResult)(unsafe.Pointer(uintptr(unsafe.Pointer(results)) + uintptr(i)*unsafe.Sizeof(*results)))
        if result.body != nil {
            output[i] = C.GoBytes(unsafe.Pointer(result.body), C.int(result.body_len))
        }
    }
    
    return output, nil
}

// Ping sends a ping request
func (t *Tunnel) Ping() error {
    ret := C.ky_tunnel_ping(t.handle)
    if ret != C.KY_TUNNEL_OK {
        return errors.New("ping failed")
    }
    return nil
}

// Close closes the tunnel connection
func (t *Tunnel) Close() error {
    if t.handle != nil {
        C.ky_tunnel_destroy(t.handle)
        t.handle = nil
    }
    return nil
}
