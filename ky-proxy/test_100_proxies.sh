#!/bin/bash
echo "🧪 Testing 100 different proxies..."

# Test proxy with different IPs
for i in {1..100}; do
    echo "Proxy #$i:"
    curl -s -x http://127.0.0.1:8080 http://httpbin.org/ip | jq -r '.origin' 2>/dev/null || echo "Failed"
    sleep 0.5
done
