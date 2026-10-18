#!/bin/bash
echo "🧪 Testing ky-proxy with fast servers..."

# Test 1: Cloudflare 10MB
echo "=== Cloudflare 10MB Test ==="
time curl -x http://127.0.0.1:8080 "https://speed.cloudflare.com/__down?bytes=10485760" -o /dev/null

# Test 2: Cloudflare 50MB
echo "=== Cloudflare 50MB Test ==="
time curl -x http://127.0.0.1:8080 "https://speed.cloudflare.com/__down?bytes=52428800" -o /dev/null

# Test 3: Hetzner 10MB
echo "=== Hetzner 10MB Test ==="
time curl -x http://127.0.0.1:8080 "https://speed.hetzner.de/10MB.bin" -o /dev/null

# Test 4: Hetzner 100MB
echo "=== Hetzner 100MB Test ==="
time curl -x http://127.0.0.1:8080 "https://speed.hetzner.de/100MB.bin" -o /dev/null

# Test 5: OVH 10MB
echo "=== OVH 10MB Test ==="
time curl -x http://127.0.0.1:8080 "http://proof.ovh.net/files/10Mb.dat" -o /dev/null

echo "✅ Tests complete!"
