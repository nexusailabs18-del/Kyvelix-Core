#!/bin/bash
echo "🧪 Testing Virtual Network Interfaces with Generated IPs"

# Check if we can create virtual interfaces
echo "=== Current interfaces ==="
ip link show | grep -E "^[0-9]+:"

# Try to create a dummy interface
echo -e "\n=== Creating dummy interface ==="
sudo ip link add dummy0 type dummy 2>&1
sudo ip addr add 10.0.0.1/24 dev dummy0 2>&1
sudo ip link set dummy0 up 2>&1
echo "✅ Created dummy0 with 10.0.0.1"

# Show it
echo -e "\n=== Dummy interface ==="
ip addr show dummy0

# Try to route through it
echo -e "\n=== Routing test ==="
curl --interface dummy0 http://httpbin.org/ip 2>&1

echo -e "\n=== Cleanup ==="
sudo ip link delete dummy0 2>&1
