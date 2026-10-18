#!/bin/bash
echo "🧪 Testing ky-proxy..."

# Run the proxy generator and capture output
cargo run --release &
PROXY_PID=$!

# Wait for startup
sleep 2

# Test with curl using a proxy from our pool
echo "Testing proxy connection..."
curl -x http://127.0.0.1:8080 http://httpbin.org/ip

# Kill the proxy process
kill $PROXY_PID
