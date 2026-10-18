#!/bin/bash
# ky-proxy Control Interface

show_status() {
    echo "=== ky-proxy STATUS ==="
    echo "Server: $(pgrep ky-proxy > /dev/null && echo '✅ Running' || echo '❌ Stopped')"
    echo "Port: 8080"
    echo ""
    echo "=== TOGGLES ==="
    echo "1. TLS Spoofing: ON"
    echo "2. Random Headers: ON"
    echo "3. Random UA: ON"
    echo "4. Random Cookies: ON"
    echo "5. Random Geo: ON"
    echo "6. Random Behavior: ON"
    echo "7. Smart Filtering: ON"
    echo "8. AI Generation: ON"
    echo "9. Rate Limiting: ON"
    echo "10. Auth Required: ON"
    echo ""
    echo "=== SCRAPE MODE ==="
    echo "Current: Full"
    echo "Options: full, text-only, images-only, minimal"
}

start_server() {
    ./target/debug/ky-proxy &
    echo "✅ Server started on 127.0.0.1:8080"
}

stop_server() {
    pkill ky-proxy
    echo "✅ Server stopped"
}

case "$1" in
    start) start_server ;;
    stop) stop_server ;;
    status) show_status ;;
    restart) stop_server; sleep 1; start_server ;;
    *) 
        echo "Usage: ./control_proxy.sh [start|stop|status|restart]"
        ;;
esac
