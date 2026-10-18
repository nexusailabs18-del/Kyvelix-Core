from playwright.sync_api import sync_playwright

print("🧪 Testing with REAL Chrome browser...")

with sync_playwright() as p:
    browser = p.chromium.launch(
        proxy={
            "server": "http://127.0.0.1:8080",
            "username": "admin",
            "password": "kyprox123"
        },
        headless=True  # Headless for terminal
    )
    page = browser.new_page()
    
    print("📥 Loading Akamai...")
    page.goto("https://www.akamai.com", timeout=30000)
    print(f"✅ Title: {page.title()}")
    
    print("\n📥 Loading Amazon...")
    page.goto("https://www.amazon.com", timeout=30000)
    print(f"✅ Title: {page.title()}")
    
    print("\n📥 Loading Cloudflare...")
    page.goto("https://www.cloudflare.com", timeout=30000)
    print(f"✅ Title: {page.title()}")
    
    browser.close()
    print("\n🎉 Real browser test complete!")
