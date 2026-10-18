#!/bin/bash
echo "🧪 Testing Bypass Rate (With Auth)"
echo "=============================================="

# Auth credentials
AUTH="admin:kyprox123"

declare -a sites=(
    "https://www.amazon.com"
    "https://www.cloudflare.com"
    "https://www.akamai.com"
    "https://www.datadome.co"
    "https://www.perimeterx.com"
    "https://www.imperva.com"
    "https://www.radware.com"
    "https://www.f5.com"
    "https://www.hcaptcha.com"
    "https://www.recaptcha.net"
)

total=0
success=0
blocked=0

for site in "${sites[@]}"; do
    total=$((total + 1))
    echo "Testing: $site"
    
    status=$(curl -s -x http://127.0.0.1:8080 \
        --proxy-user "$AUTH" \
        -H "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36" \
        "$site" -o /dev/null -w "%{http_code}" --max-time 10)
    
    if [ "$status" = "200" ] || [ "$status" = "301" ] || [ "$status" = "302" ]; then
        echo "  ✅ PASSED (Status: $status)"
        success=$((success + 1))
    elif [ "$status" = "403" ] || [ "$status" = "429" ]; then
        echo "  ❌ BLOCKED (Status: $status)"
        blocked=$((blocked + 1))
    else
        echo "  ⚠️ OTHER (Status: $status)"
    fi
    echo ""
done

echo "=============================================="
echo "📊 RESULTS"
echo "Total Sites: $total"
echo "✅ Passed: $success"
echo "❌ Blocked: $blocked"
echo "Bypass Rate: $((success * 100 / total))%"
echo "=============================================="
