package kytunnel

import (
    "net"
    "sync"
    "time"
)

// Dialer manages connection pool and DNS cache
type Dialer struct {
    mu          sync.Mutex
    connections map[string]*Tunnel
    dnsCache    map[string]string
    maxConns    int
}

// NewDialer creates a new connection pool dialer
func NewDialer(maxConns int) *Dialer {
    return &Dialer{
        connections: make(map[string]*Tunnel),
        dnsCache:    make(map[string]string),
        maxConns:    maxConns,
    }
}

// GetTunnel returns an existing tunnel or creates a new one
func (d *Dialer) GetTunnel(addr string) (*Tunnel, error) {
    d.mu.Lock()
    defer d.mu.Unlock()
    
    // Check if we have an existing connection
    if tunnel, ok := d.connections[addr]; ok {
        // Verify connection is still alive
        if err := tunnel.Ping(); err == nil {
            return tunnel, nil
        }
        // Connection is dead, close it
        tunnel.Close()
        delete(d.connections, addr)
    }
    
    // Create new tunnel
    tunnel, err := NewTunnel(addr)
    if err != nil {
        return nil, err
    }
    
    // Store in pool if we have space
    if len(d.connections) < d.maxConns {
        d.connections[addr] = tunnel
    }
    
    return tunnel, nil
}

// ResolveDNS resolves a hostname with caching
func (d *Dialer) ResolveDNS(host string) (string, error) {
    d.mu.Lock()
    defer d.mu.Unlock()
    
    // Check cache first
    if ip, ok := d.dnsCache[host]; ok {
        return ip, nil
    }
    
    // Resolve
    addrs, err := net.LookupHost(host)
    if err != nil {
        return "", err
    }
    
    if len(addrs) > 0 {
        d.dnsCache[host] = addrs[0]
        return addrs[0], nil
    }
    
    return "", net.ErrWriteToConnected
}

// ClearCache clears the DNS cache
func (d *Dialer) ClearCache() {
    d.mu.Lock()
    defer d.mu.Unlock()
    d.dnsCache = make(map[string]string)
}

// CloseAll closes all connections in the pool
func (d *Dialer) CloseAll() {
    d.mu.Lock()
    defer d.mu.Unlock()
    
    for addr, tunnel := range d.connections {
        tunnel.Close()
        delete(d.connections, addr)
    }
}

// Stats returns connection pool statistics
func (d *Dialer) Stats() (int, int, time.Duration) {
    d.mu.Lock()
    defer d.mu.Unlock()
    
    return len(d.connections), d.maxConns, 0
}
