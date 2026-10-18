package main

import (
	"context"
	"fmt"
	"math/rand"
	"strings"

	"github.com/redis/go-redis/v9"
)

var redisClient *redis.Client
var ctx = context.Background()

var chromeUserAgents = []string{
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
	"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
}

var firefoxUserAgents = []string{
	"Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0",
	"Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:126.0) Gecko/20100101 Firefox/126.0",
	"Mozilla/5.0 (X11; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0",
}

func initRedis() error {
	redisClient = redis.NewClient(&redis.Options{
		Addr: "localhost:6379",
	})
	return redisClient.Ping(ctx).Err()
}

func loadHeadersToRedis() error {
	if redisClient == nil {
		if err := initRedis(); err != nil {
			return err
		}
	}

	pipe := redisClient.Pipeline()

	for i, ua := range chromeUserAgents {
		pipe.SAdd(ctx, "ua:chrome", ua)
		pipe.HSet(ctx, "headers:chrome", fmt.Sprintf("ua_%d", i), ua)
	}

	for i, ua := range firefoxUserAgents {
		pipe.SAdd(ctx, "ua:firefox", ua)
		pipe.HSet(ctx, "headers:firefox", fmt.Sprintf("ua_%d", i), ua)
	}

	pipe.SAdd(ctx, "accept_languages", "en-US,en;q=0.9", "en-GB,en;q=0.9", "en-CA,en;q=0.9", "en-IN,en;q=0.9")
	pipe.SAdd(ctx, "accept_encodings", "gzip, deflate, br", "gzip, deflate", "gzip, br")

	_, err := pipe.Exec(ctx)
	return err
}

func getRandomUserAgent(profile string) string {
	if redisClient == nil {
		initRedis()
	}

	set := "ua:chrome"
	if profile == "firefox" {
		set = "ua:firefox"
	}

	ua, err := redisClient.SRandMember(ctx, set).Result()
	if err != nil || ua == "" {
		// Fallback
		if profile == "firefox" && len(firefoxUserAgents) > 0 {
			return firefoxUserAgents[rand.Intn(len(firefoxUserAgents))]
		}
		return chromeUserAgents[rand.Intn(len(chromeUserAgents))]
	}
	return ua
}

func getRandomAcceptLanguage() string {
	if redisClient == nil {
		initRedis()
	}
	lang, err := redisClient.SRandMember(ctx, "accept_languages").Result()
	if err != nil {
		return "en-US,en;q=0.9"
	}
	return lang
}

func getRandomizedHeaders(profile string) map[string]string {
	headers := make(map[string]string)

	headers["User-Agent"] = getRandomUserAgent(profile)
	headers["Accept"] = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
	headers["Accept-Language"] = getRandomAcceptLanguage()
	headers["Accept-Encoding"] = "gzip, deflate, br"
	headers["Cache-Control"] = "no-cache"
	headers["Pragma"] = "no-cache"
	headers["Sec-Fetch-Dest"] = "document"
	headers["Sec-Fetch-Mode"] = "navigate"
	headers["Sec-Fetch-Site"] = "none"
	headers["Upgrade-Insecure-Requests"] = "1"

	return headers
}

func getRandomHeaderSetFromRedis(profile string) map[string]string {
	if redisClient == nil {
		initRedis()
	}

	headers := getRandomizedHeaders(profile)

	// Add random cookies if available
	cookieKey := "cookies:" + profile + ":*"
	keys, _ := redisClient.Keys(ctx, cookieKey).Result()
	if len(keys) > 0 {
		randomCookie, _ := redisClient.Get(ctx, keys[rand.Intn(len(keys))]).Result()
		if randomCookie != "" {
			headers["Cookie"] = randomCookie
		}
	}

	return headers
}

func getUserAgentCount() int {
	if redisClient == nil {
		initRedis()
	}
	chromeCount, _ := redisClient.SCard(ctx, "ua:chrome").Result()
	firefoxCount, _ := redisClient.SCard(ctx, "ua:firefox").Result()
	return int(chromeCount + firefoxCount)
}

func printHeaderStats() {
	if redisClient == nil {
		initRedis()
	}
	fmt.Println("=== HEADER POOL STATS ===")
	fmt.Printf("Chrome UAs: %d\n", getUserAgentCount())
	fmt.Printf("Accept-Languages: %d\n", redisClient.SCard(ctx, "accept_languages").Val())
	fmt.Printf("Firefox UAs: %d\n", redisClient.SCard(ctx, "ua:firefox").Val())
	fmt.Println()
}

func sanitizeHeaders(headers map[string]string) map[string]string {
	safe := make(map[string]string)
	for key, value := range headers {
		cleanKey := strings.TrimSpace(key)
		cleanValue := strings.TrimSpace(value)
		if cleanKey != "" && cleanValue != "" {
			safe[cleanKey] = cleanValue
		}
	}
	return safe
}
