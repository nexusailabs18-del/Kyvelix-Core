#!/bin/bash
echo "🚀 Starting ky-proxy stress test..."

# Test 1: Sequential requests (50 requests)
echo "=== Test 1: 50 Sequential Requests ==="
time for i in {1..50}; do
    curl -s -x http://127.0.0.1:8080 http://httpbin.org/ip -o /dev/null &
done
wait
echo "✅ Sequential test complete"

# Test 2: Concurrent requests (20 parallel)
echo "=== Test 2: 20 Concurrent Requests ==="
time for i in {1..20}; do
    curl -s -x http://127.0.0.1:8080 "http://httpbin.org/get?id=$i" -o /dev/null &
done
wait
echo "✅ Concurrent test complete"

# Test 3: Mixed HTTP/HTTPS
echo "=== Test 3: Mixed HTTP/HTTPS ==="
time {
    # HTTP requests
    for i in {1..10}; do
        curl -s -x http://127.0.0.1:8080 http://httpbin.org/ip -o /dev/null &
    done
    # HTTPS requests
    for i in {1..10}; do
        curl -s -x http://127.0.0.1:8080 https://httpbin.org/ip -o /dev/null &
    done
    wait
}
echo "✅ Mixed test complete"

# Test 4: Large payload POST
echo "=== Test 4: POST with Data ==="
time curl -s -x http://127.0.0.1:8080 \
    -X POST http://httpbin.org/post \
    -H "Content-Type: application/json" \
    -d '{"test":"data","size":"1MB"}' \
    -o /dev/null
echo "✅ POST test complete"

# Test 5: Connection reuse
echo "=== Test 5: Connection Reuse ==="
time for i in {1..30}; do
    curl -s -x http://127.0.0.1:8080 http://example.com -o /dev/null
done
echo "✅ Connection reuse test complete"

echo "🎉 All stress tests complete!"
