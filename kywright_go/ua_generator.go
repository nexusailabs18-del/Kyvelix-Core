package main

import (
	"fmt"
	"math/rand"
	"strings"
)

var chromeVersions = []string{
	"120.0.0.0", "121.0.0.0", "122.0.0.0", "123.0.0.0", "124.0.0.0",
	"125.0.0.0", "126.0.0.0", "127.0.0.0", "128.0.0.0", "129.0.0.0",
	"130.0.0.0", "131.0.0.0", "132.0.0.0", "133.0.0.0", "134.0.0.0",
	"135.0.0.0", "136.0.0.0", "137.0.0.0", "138.0.0.0", "139.0.0.0",
	"140.0.0.0", "141.0.0.0", "142.0.0.0", "143.0.0.0", "144.0.0.0",
	"145.0.0.0", "146.0.0.0", "147.0.0.0", "148.0.0.0", "149.0.0.0",
	"150.0.0.0",
}

var firefoxVersions = []string{
	"122.0", "123.0", "124.0", "125.0", "126.0", "127.0", "128.0", "129.0",
	"130.0", "131.0", "132.0", "133.0", "134.0", "135.0", "136.0", "137.0",
	"138.0", "139.0", "140.0",
}

var windowsVersions = []string{
	"Windows NT 10.0; Win64; x64",
	"Windows NT 10.0; Win64; x64; rv:",
	"Windows NT 6.1; Win64; x64",
	"Windows NT 6.3; Win64; x64",
}

var macVersions = []string{
	"Macintosh; Intel Mac OS X 10_15_7",
	"Macintosh; Intel Mac OS X 11_6_5",
	"Macintosh; Intel Mac OS X 12_7_4",
	"Macintosh; Intel Mac OS X 13_6_6",
	"Macintosh; Intel Mac OS X 14_4_1",
}

var linuxVersions = []string{
	"X11; Linux x86_64",
	"X11; Linux x86_64; rv:",
	"X11; Ubuntu; Linux x86_64",
	"X11; Fedora; Linux x86_64",
}

var androidVersions = []string{
	"Linux; Android 10; K",
	"Linux; Android 11; Pixel 5",
	"Linux; Android 12; SM-G991B",
	"Linux; Android 13; Pixel 7",
	"Linux; Android 14; Pixel 8",
}

func generateChromeUserAgents() []string {
	var uas []string
	seen := make(map[string]bool)

	for _, chromeVer := range chromeVersions {
		for _, os := range windowsVersions {
			if strings.Contains(os, "rv:") {
				continue
			}
			ua := fmt.Sprintf("Mozilla/5.0 (%s) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", os, chromeVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
		for _, os := range macVersions {
			ua := fmt.Sprintf("Mozilla/5.0 (%s) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", os, chromeVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
		for _, os := range linuxVersions {
			if strings.Contains(os, "rv:") {
				continue
			}
			ua := fmt.Sprintf("Mozilla/5.0 (%s) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Safari/537.36", os, chromeVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
	}

	return uas
}

func generateFirefoxUserAgents() []string {
	var uas []string
	seen := make(map[string]bool)

	for _, fxVer := range firefoxVersions {
		for _, os := range windowsVersions {
			ua := fmt.Sprintf("Mozilla/5.0 (%s; rv:%s) Gecko/20100101 Firefox/%s", os, fxVer, fxVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
		for _, os := range macVersions {
			ua := fmt.Sprintf("Mozilla/5.0 (%s; rv:%s) Gecko/20100101 Firefox/%s", os, fxVer, fxVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
		for _, os := range linuxVersions {
			ua := fmt.Sprintf("Mozilla/5.0 (%s; rv:%s) Gecko/20100101 Firefox/%s", os, fxVer, fxVer)
			if !seen[ua] {
				seen[ua] = true
				uas = append(uas, ua)
			}
		}
	}

	return uas
}

func generateMobileUserAgents() []string {
	var uas []string
	for _, android := range androidVersions {
		for _, chromeVer := range []string{"120.0.0.0", "124.0.0.0", "131.0.0.0", "150.0.0.0"} {
			ua := fmt.Sprintf("Mozilla/5.0 (%s) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/%s Mobile Safari/537.36", android, chromeVer)
			uas = append(uas, ua)
		}
	}
	return uas
}

func buildLargeHeaderPool() ([]string, []string) {
	chromeUAs := generateChromeUserAgents()
	firefoxUAs := generateFirefoxUserAgents()
	mobileUAs := generateMobileUserAgents()

	allChrome := append(chromeUAs, mobileUAs...)

	return allChrome, firefoxUAs
}

func loadLargeHeaderPoolToRedis() error {
	if redisClient == nil {
		if err := initRedis(); err != nil {
			return err
		}
	}

	chromeUAs, firefoxUAs := buildLargeHeaderPool()

	pipe := redisClient.Pipeline()

	for _, ua := range chromeUAs {
		pipe.SAdd(ctx, "ua:chrome", ua)
	}

	for _, ua := range firefoxUAs {
		pipe.SAdd(ctx, "ua:firefox", ua)
	}

	// Add more accept languages
	languages := []string{
		"en-US,en;q=0.9", "en-GB,en;q=0.9", "en-CA,en;q=0.9",
		"en-AU,en;q=0.9", "en-IN,en;q=0.8", "en-NZ,en;q=0.9",
		"en-IE,en;q=0.9", "en-ZA,en;q=0.9", "en-SG,en;q=0.9",
		"en-HK,en;q=0.9",
	}
	for _, lang := range languages {
		pipe.SAdd(ctx, "accept_languages", lang)
	}

	encodings := []string{
		"gzip, deflate, br", "gzip, deflate", "gzip, br",
		"gzip, deflate, zstd", "gzip, deflate, br, zstd",
	}
	for _, enc := range encodings {
		pipe.SAdd(ctx, "accept_encodings", enc)
	}

	_, err := pipe.Exec(ctx)
	return err
}

func generateRandomUA() string {
	chromeUAs, _ := buildLargeHeaderPool()
	return chromeUAs[rand.Intn(len(chromeUAs))]
}
