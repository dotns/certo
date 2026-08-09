package certo

import (
	"database/sql"
)

type DB interface {
	// TXT record management
	PresentTXT(fqdn, value string) error
	CleanupTXT(fqdn, value string) error
	GetTXTForDomain(string) ([]string, error)
	ListTXTRecords() ([]TXTRecord, error)
	ListTXTRecordsByDomains(domains []string) ([]TXTRecord, error)
	// UpdateACMEDNSTXT stores a TXT value for an acme-dns subdomain, keeping only the
	// two most recent values for the domain (acme-dns rolling-record semantics).
	UpdateACMEDNSTXT(fqdn, value string) error

	// User management
	CreateUser(username, passwordHash string) (User, error)
	// CreateACMEDNSAccount allocates an anonymous acme-dns account: a fresh "acme-<nanoid>"
	// user with a default API key (the acme-dns password) bound to a random subdomain, with
	// an optional source-IP allow list (CIDRs). Returns the username, API key, and subdomain.
	CreateACMEDNSAccount(allowFrom []string) (username, apiKey, subdomain string, err error)
	GetUserByUsername(username string) (User, error)
	GetUserByID(id int64) (User, error)

	// Domain management
	AddUserDomain(userID int64, username, domain string) (UserDomain, error)
	RemoveUserDomain(userID int64, domain string) error
	GetUserDomains(userID int64) ([]UserDomain, error)
	GetSubdomainByUserDomain(userID int64, domain string) (string, error)
	GetSubdomainOwner(subdomain string) (int64, error)
	GetUserDomainBySubdomain(subdomain string) (userID int64, domain string, allowFrom []string, err error)

	// API Key management
	CreateAPIKey(userID int64, name string, scope []string) (APIKey, error)
	ListAPIKeys(userID int64) ([]APIKey, error)
	DeleteAPIKey(userID, keyID int64) error
	GetAPIKeyByValue(keyValue string) (APIKey, error)
	UpdateAPIKeyScope(keyID int64, scope []string) error

	// Admin operations
	ListUsers() ([]User, error)
	DeleteUser(userID int64) error
	ListAllDomains() ([]UserDomain, error)

	GetBackend() *sql.DB
	SetBackend(*sql.DB)
	Close()
}

type NS interface {
	Start(errorChannel chan error)
	SetOwnAuthKey(key string)
	SetNotifyStartedFunc(func())
	ParseRecords()
}
