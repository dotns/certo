package certo

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"math/big"
	"strings"
)

const nanoidAlphabet = "0123456789abcdefghijklmnopqrstuvwxyz"
const subdomainLength = 8
const apiKeyLength = 32

// ACMEDNSSubdomainLength is the length of the random subdomain allocated for an anonymous
// acme-dns /register account. It is deliberately longer than subdomainLength (the
// deterministic 8-char subdomains) so the two pools are easy to tell apart.
const ACMEDNSSubdomainLength = 10

// ACMEDNSUsernamePrefix prefixes the auto-generated username for acme-dns /register accounts.
const ACMEDNSUsernamePrefix = "acme-"

// acmeDNSUsernameLength is the random part length of an acme-dns account username.
const acmeDNSUsernameLength = 16

// GenerateNanoID returns a random length-char lowercase-alphanumeric id.
func GenerateNanoID(length int) string {
	b := make([]byte, length)
	max := big.NewInt(int64(len(nanoidAlphabet)))
	for i := range b {
		n, _ := rand.Int(rand.Reader, max)
		b[i] = nanoidAlphabet[n.Int64()]
	}
	return string(b)
}

// GenerateAPIKey creates a random API key (32-char lowercase alphanumeric).
func GenerateAPIKey() string {
	return GenerateNanoID(apiKeyLength)
}

// GenerateACMEDNSUsername creates a random "acme-<nanoid>" username for an anonymous
// acme-dns registration, distinct from human-chosen dashboard usernames.
func GenerateACMEDNSUsername() string {
	return ACMEDNSUsernamePrefix + GenerateNanoID(acmeDNSUsernameLength)
}

// GenerateSubdomain creates a deterministic subdomain from username and domain.
// The same username+domain+salt always produces the same subdomain.
// salt=0 is the default; increment salt to resolve collisions.
func GenerateSubdomain(username, domain string, salt int) string {
	input := strings.ToLower(username) + ":" + strings.ToLower(domain)
	if salt > 0 {
		input += ":" + strings.Repeat("x", salt)
	}
	hash := sha256.Sum256([]byte(input))
	hexStr := hex.EncodeToString(hash[:])
	return hexStr[:subdomainLength]
}

// InternalDomain returns the full internal domain for a subdomain under the base domain.
// Example: subdomain "a7x9k2", baseDomain "dns.example.com" -> "a7x9k2.dns.example.com"
func InternalDomain(subdomain, baseDomain string) string {
	baseDomain = strings.TrimSuffix(strings.TrimSpace(baseDomain), ".")
	return subdomain + "." + baseDomain
}

// ExtractDomainFromFQDN strips _acme-challenge. prefix and trailing dot.
// Example: "_acme-challenge.example.com." -> "example.com"
func ExtractDomainFromFQDN(fqdn string) string {
	fqdn = strings.ToLower(strings.TrimSpace(fqdn))
	fqdn = strings.TrimSuffix(fqdn, ".")
	fqdn = strings.TrimPrefix(fqdn, "_acme-challenge.")
	return fqdn
}

// ExtractSubdomainFromFQDN extracts the nanoid subdomain if the FQDN is under the base domain.
// Example: "r0hc4bc6.s.example.com." with baseDomain "s.example.com" -> "r0hc4bc6", true
// Returns empty string and false if the FQDN is not under the base domain.
func ExtractSubdomainFromFQDN(fqdn, baseDomain string) (string, bool) {
	fqdn = strings.ToLower(strings.TrimSpace(fqdn))
	fqdn = strings.TrimSuffix(fqdn, ".")
	baseDomain = strings.ToLower(strings.TrimSuffix(strings.TrimSpace(baseDomain), "."))
	suffix := "." + baseDomain
	if strings.HasSuffix(fqdn, suffix) {
		sub := strings.TrimSuffix(fqdn, suffix)
		if sub != "" && !strings.Contains(sub, ".") {
			return sub, true
		}
	}
	return "", false
}
