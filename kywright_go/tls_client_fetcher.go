package main

import (
	"io"
	"strings"

	fhttp "github.com/bogdanfinn/fhttp"
	http_client "github.com/bogdanfinn/tls-client"
	"github.com/bogdanfinn/tls-client/profiles"
)

var tlsClient http_client.HttpClient

var profileHeaders = map[string]map[string]string{
	"chrome_120": {
		"User-Agent":      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
		"Accept":          "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
		"Accept-Language": "en-US,en;q=0.9",
	},
	"chrome_124": {
		"User-Agent":      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
		"Accept":          "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
		"Accept-Language": "en-US,en;q=0.9",
	},
	"firefox_126": {
		"User-Agent":      "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0",
		"Accept":          "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
		"Accept-Language": "en-US,en;q=0.5",
	},
}

func initTLSClient() error {
	client, err := http_client.NewHttpClient(
		http_client.NewNoopLogger(),
		[]http_client.HttpClientOption{
			http_client.WithClientProfile(profiles.Chrome_120),
			http_client.WithTimeoutSeconds(15),
			http_client.WithProxyUrl("http://admin:kyprox123@127.0.0.1:8081"),
		}...,
	)
	if err != nil {
		return err
	}
	tlsClient = client
	return nil
}

func tlsFetchWithProfile(rawURL, profileName string) (string, map[string]string, error) {
	// Try KY Tunnel first (fast path)

	if tlsClient == nil {
		if err := initTLSClient(); err != nil {
			return "", nil, err
		}
	}

	req, err := fhttp.NewRequest("GET", rawURL, nil)
	if err != nil {
		return "", nil, err
	}

	var headers map[string]string
	if redisClient != nil {
		headers = getRandomizedHeaders(profileName)
	} else {
		headers = profileHeaders[profileName]
		if headers == nil {
			headers = profileHeaders["chrome_120"]
		}
	}

	for key, value := range headers {
		req.Header.Set(key, value)
	}

	resp, err := tlsClient.Do(req)
	if err != nil {
		return "", nil, err
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	cookies := make(map[string]string)
	for _, cookie := range resp.Cookies() {
		cookies[cookie.Name] = cookie.Value
	}

	return string(body), cookies, nil
}

func tlsFetch(rawURL string) (string, map[string]string, error) {
	profile := getNextProfile()
	return tlsFetchWithProfile(rawURL, profile)
}

func isBlocked(html string) bool {
	lower := strings.ToLower(html)
	return len(html) < 500 ||
		strings.Contains(lower, "captcha") ||
		strings.Contains(lower, "access denied") ||
		strings.Contains(lower, "cf-challenge") ||
		strings.Contains(lower, "just a moment") ||
		strings.Contains(lower, "enable javascript")
}
