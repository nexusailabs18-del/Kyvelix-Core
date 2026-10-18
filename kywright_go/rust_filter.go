package main

import (
	"kywright-go/kywrightfilter"
)

func rustFilter(html string) string {
	// Fast path: try local Rust filter directly
	clean, err := kywrightfilter.FilterHTML(html)
	if err == nil && clean != "" {
		return clean
	}

	// Fallback: use KY Tunnel internal bridge
	if tunnel, err := getOrCreateTunnel(); err == nil {
		bodyBytes, terr := tunnel.Fetch("ky://filter/" + html)
		if terr == nil && len(bodyBytes) > 0 {
			return string(bodyBytes)
		}
	}

	return html
}
