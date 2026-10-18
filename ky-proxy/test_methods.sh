#!/bin/bash
echo "🔍 Testing HTTP methods..."

# GET request
echo "=== GET ==="
curl -s -x http://127.0.0.1:8080 http://httpbin.org/get | jq '.method'

# POST request
echo "=== POST ==="
curl -s -x http://127.0.0.1:8080 -X POST http://httpbin.org/post -d "data=test" | jq '.method'

# PUT request
echo "=== PUT ==="
curl -s -x http://127.0.0.1:8080 -X PUT http://httpbin.org/put -d "data=test" | jq '.method'

# DELETE request
echo "=== DELETE ==="
curl -s -x http://127.0.0.1:8080 -X DELETE http://httpbin.org/delete | jq '.method'

# HEAD request
echo "=== HEAD ==="
curl -I -x http://127.0.0.1:8080 http://httpbin.org/get

# OPTIONS request
echo "=== OPTIONS ==="
curl -s -x http://127.0.0.1:8080 -X OPTIONS http://httpbin.org/get -i | head -5
