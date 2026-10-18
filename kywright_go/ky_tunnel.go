package main

/*
#cgo CFLAGS: -I../ky-tunnel/include
#cgo LDFLAGS: -L../ky-tunnel/build -lky_tunnel
#include <stdlib.h>
#include "ky_tunnel.h"
*/
import "C"
import (
	"errors"
	"fmt"
	"unsafe"
)

// Tunnel is a safe Go wrapper around the C KyTunnel.
type Tunnel struct {
	handle *C.KyTunnel
}

// NewTunnel creates a tunnel instance.
func NewTunnel(addr string) (*Tunnel, error) {
	cfg := C.CString("host=" + addr + ";port=9090;timeout_ms=5000")
	defer C.free(unsafe.Pointer(cfg))

	h := C.ky_tunnel_create(cfg)
	if h == nil {
		return nil, errors.New("failed to create tunnel")
	}

	return &Tunnel{handle: h}, nil
}

// Fetch fetches one URL through the tunnel.
func (t *Tunnel) Fetch(url string) ([]byte, error) {
	cURL := C.CString(url)
	defer C.free(unsafe.Pointer(cURL))

	var body *C.char
	var bodyLen C.size_t

	rc := C.ky_tunnel_fetch(t.handle, cURL, &body, &bodyLen)
	if rc != C.KY_TUNNEL_OK || body == nil {
		return nil, fmt.Errorf("tunnel fetch failed: code=%d bodyLen=%d", int(rc), int(bodyLen))
	}
	defer C.free(unsafe.Pointer(body))

	return C.GoBytes(unsafe.Pointer(body), C.int(bodyLen)), nil
}

// Batch fetches multiple URLs.
func (t *Tunnel) Batch(urls []string) ([][]byte, error) {
	if len(urls) == 0 {
		return nil, errors.New("empty url list")
	}

	// Build C string array
	cURLs := make([]*C.char, len(urls))
	defer func() {
		for _, p := range cURLs {
			if p != nil {
				C.free(unsafe.Pointer(p))
			}
		}
	}()

	for i, u := range urls {
		cURLs[i] = C.CString(u)
	}

	var results *C.KyTunnelResult
	rc := C.ky_tunnel_batch(t.handle, &cURLs[0], C.int(len(urls)), &results)
	if rc != C.KY_TUNNEL_OK || results == nil {
		return nil, errors.New("tunnel batch failed")
	}
	defer C.ky_tunnel_free_results(results, C.int(len(urls)))

	// Convert C array to Go slice safely
	out := make([][]byte, len(urls))
	resultSlice := unsafe.Slice(results, len(urls))

	for i, r := range resultSlice {
		if r.body != nil && r.body_len > 0 {
			out[i] = C.GoBytes(unsafe.Pointer(r.body), C.int(r.body_len))
		}
	}

	return out, nil
}

// Ping checks tunnel liveness.
func (t *Tunnel) Ping() error {
	if t.handle == nil {
		return errors.New("tunnel not initialized")
	}
	rc := C.ky_tunnel_ping(t.handle)
	if rc != C.KY_TUNNEL_OK {
		return errors.New("tunnel ping failed")
	}
	return nil
}

// Close destroys the tunnel.
func (t *Tunnel) Close() {
	if t.handle != nil {
		C.ky_tunnel_destroy(t.handle)
		t.handle = nil
	}
}
