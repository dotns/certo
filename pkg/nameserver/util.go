package nameserver

import "strings"

// sanitizeDomainQuestion normalizes a DNS question name: lowercase, strip trailing dot
func sanitizeDomainQuestion(d string) string {
	dom := strings.ToLower(d)
	dom = strings.TrimSuffix(dom, ".")
	return dom
}
