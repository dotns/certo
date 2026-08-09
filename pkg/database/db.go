package database

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"

	_ "modernc.org/sqlite"

	"go.uber.org/zap"

	"github.com/dotns/certo/pkg/certo"
)

type acmednsdb struct {
	DB     *sql.DB
	Mutex  sync.Mutex
	Logger *zap.SugaredLogger
	Config *certo.Config
}

// DBVersion shows the current database schema version.
// v2 added user_domains.allowfrom (acme-dns source-IP allow list).
var DBVersion = 2

var acmeTable = `
	CREATE TABLE IF NOT EXISTS acmedns(
		Name TEXT,
		Value TEXT
	);`

var txtTable = `
	CREATE TABLE IF NOT EXISTS txt(
		Domain TEXT NOT NULL,
		Value TEXT NOT NULL DEFAULT '',
		LastUpdate INT
	);`

var usersTable = `
	CREATE TABLE IF NOT EXISTS users(
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		username TEXT UNIQUE NOT NULL,
		password_hash TEXT NOT NULL,
		created_at INT NOT NULL
	);`

var userDomainsTable = `
	CREATE TABLE IF NOT EXISTS user_domains(
		user_id INTEGER NOT NULL,
		domain TEXT NOT NULL,
		subdomain TEXT UNIQUE NOT NULL,
		allowfrom TEXT NOT NULL DEFAULT '[]',
		UNIQUE(user_id, domain),
		FOREIGN KEY(user_id) REFERENCES users(id)
	);`

var apiKeysTable = `
	CREATE TABLE IF NOT EXISTS api_keys(
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		user_id INTEGER NOT NULL,
		name TEXT NOT NULL,
		key_value TEXT UNIQUE NOT NULL,
		scope TEXT NOT NULL DEFAULT '["*"]',
		created_at INT NOT NULL,
		FOREIGN KEY(user_id) REFERENCES users(id)
	);`

func Init(config *certo.Config, logger *zap.SugaredLogger) (certo.DB, error) {
	var d = &acmednsdb{Config: config, Logger: logger}
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	if config.Database.Engine != "" && config.Database.Engine != "sqlite" {
		return d, fmt.Errorf("invalid database engine %q: only sqlite is supported", config.Database.Engine)
	}
	dbPath, err := sqliteFilePath(config.Database.Connection)
	if err != nil {
		return d, err
	}
	if dbPath != "" {
		if dir := filepath.Dir(dbPath); dir != "" && dir != "." {
			_ = os.MkdirAll(dir, 0755)
		}
	}
	db, err := sql.Open("sqlite", config.Database.Connection)
	if err != nil {
		return d, err
	}
	d.DB = db
	_, _ = d.DB.Exec(acmeTable)
	_, _ = d.DB.Exec(txtTable)
	_, _ = d.DB.Exec(usersTable)
	_, _ = d.DB.Exec(userDomainsTable)
	_, _ = d.DB.Exec(apiKeysTable)
	var versionString string
	_ = d.DB.QueryRow("SELECT Value FROM acmedns WHERE Name='db_version'").Scan(&versionString)
	if versionString == "" {
		insversion := fmt.Sprintf("INSERT INTO acmedns (Name, Value) values('db_version', '%d')", DBVersion)
		_, err = db.Exec(insversion)
	} else {
		err = d.checkDBUpgrades(versionString)
	}
	if err != nil {
		if closeErr := db.Close(); closeErr != nil {
			logger.Warnw("Failed to close database after init error", "error", closeErr)
		}
	}
	return d, err
}

func (d *acmednsdb) checkDBUpgrades(versionString string) error {
	version, err := strconv.Atoi(versionString)
	if err != nil {
		return err
	}
	if version > DBVersion {
		return fmt.Errorf("unsupported database version %d, expected %d; regenerate the database", version, DBVersion)
	}
	if version < DBVersion {
		return d.handleDBUpgrades(version)
	}
	return nil
}

func (d *acmednsdb) handleDBUpgrades(version int) error {
	if version == 1 {
		if err := d.upgrade1to2(); err != nil {
			return err
		}
		version = 2
	}
	return nil
}

// upgrade1to2 adds the user_domains.allowfrom column (acme-dns source-IP allow list).
func (d *acmednsdb) upgrade1to2() error {
	if _, err := d.DB.Exec(`ALTER TABLE user_domains ADD COLUMN allowfrom TEXT NOT NULL DEFAULT '[]'`); err != nil {
		return fmt.Errorf("upgrade 1->2 (add allowfrom): %w", err)
	}
	if _, err := d.DB.Exec(`UPDATE acmedns SET Value='2' WHERE Name='db_version'`); err != nil {
		return fmt.Errorf("upgrade 1->2 (bump db_version): %w", err)
	}
	return nil
}

func sqliteFilePath(connection string) (string, error) {
	if connection == "" || connection == ":memory:" {
		return "", nil
	}
	if strings.Contains(connection, "://") && !strings.HasPrefix(connection, "file:") {
		return "", fmt.Errorf("invalid sqlite connection %q: only local paths and file: URLs are supported", connection)
	}
	if strings.HasPrefix(connection, "file:") {
		u, err := url.Parse(connection)
		if err != nil {
			return strings.TrimPrefix(connection, "file:"), nil
		}
		if u.Path != "" {
			return u.Path, nil
		}
		return u.Opaque, nil
	}
	if path, _, ok := strings.Cut(connection, "?"); ok {
		return path, nil
	}
	return connection, nil
}

// --- API Key methods ---

func (d *acmednsdb) CreateAPIKey(userID int64, name string, scope []string) (certo.APIKey, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	keyValue := certo.GenerateAPIKey()
	scopeJSON, _ := json.Marshal(scope)
	now := time.Now().Unix()
	insSQL := `INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?, ?, ?, ?, ?)`
	result, err := d.DB.Exec(insSQL, userID, name, keyValue, string(scopeJSON), now)
	if err != nil {
		return certo.APIKey{}, fmt.Errorf("failed to create api key: %w", err)
	}
	id, _ := result.LastInsertId()
	return certo.APIKey{ID: id, UserID: userID, Name: name, Key: keyValue, Scope: scope, CreatedAt: now}, nil
}

func (d *acmednsdb) ListAPIKeys(userID int64) ([]certo.APIKey, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var keys []certo.APIKey
	getSQL := `SELECT id, user_id, name, key_value, scope, created_at FROM api_keys WHERE user_id=? ORDER BY created_at`
	rows, err := d.DB.Query(getSQL, userID)
	if err != nil {
		return keys, err
	}
	defer rows.Close()
	for rows.Next() {
		var k certo.APIKey
		var scopeJSON string
		if err := rows.Scan(&k.ID, &k.UserID, &k.Name, &k.Key, &scopeJSON, &k.CreatedAt); err != nil {
			return keys, err
		}
		_ = json.Unmarshal([]byte(scopeJSON), &k.Scope)
		keys = append(keys, k)
	}
	if err := rows.Err(); err != nil {
		return keys, err
	}
	return keys, nil
}

func (d *acmednsdb) DeleteAPIKey(userID, keyID int64) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	delSQL := `DELETE FROM api_keys WHERE id=? AND user_id=?`
	_, err := d.DB.Exec(delSQL, keyID, userID)
	return err
}

func (d *acmednsdb) GetAPIKeyByValue(keyValue string) (certo.APIKey, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var k certo.APIKey
	var scopeJSON string
	getSQL := `SELECT id, user_id, name, key_value, scope, created_at FROM api_keys WHERE key_value=?`
	err := d.DB.QueryRow(getSQL, keyValue).Scan(&k.ID, &k.UserID, &k.Name, &k.Key, &scopeJSON, &k.CreatedAt)
	if err != nil {
		return k, fmt.Errorf("api key not found: %w", err)
	}
	_ = json.Unmarshal([]byte(scopeJSON), &k.Scope)
	return k, nil
}

func (d *acmednsdb) UpdateAPIKeyScope(keyID int64, scope []string) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	scopeJSON, _ := json.Marshal(scope)
	updSQL := `UPDATE api_keys SET scope=? WHERE id=?`
	_, err := d.DB.Exec(updSQL, string(scopeJSON), keyID)
	return err
}

// --- TXT record methods ---

func (d *acmednsdb) PresentTXT(fqdn, value string) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain := sanitizeFQDN(fqdn)
	timenow := time.Now().Unix()
	insSQL := `INSERT INTO txt (Domain, Value, LastUpdate) VALUES (?, ?, ?)`
	sm, err := d.DB.Prepare(insSQL)
	if err != nil {
		return fmt.Errorf("failed to prepare present statement: %w", err)
	}
	defer sm.Close()
	_, err = sm.Exec(domain, value, timenow)
	return err
}

func (d *acmednsdb) CleanupTXT(fqdn, value string) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain := sanitizeFQDN(fqdn)
	delSQL := `DELETE FROM txt WHERE Domain=? AND Value=?`
	sm, err := d.DB.Prepare(delSQL)
	if err != nil {
		return fmt.Errorf("failed to prepare cleanup statement: %w", err)
	}
	defer sm.Close()
	_, err = sm.Exec(domain, value)
	return err
}

// UpdateACMEDNSTXT stores a TXT value for an acme-dns subdomain and trims the domain's
// rows to the two most recent values, matching acme-dns rolling-record behavior (clients
// never call cleanup, so unbounded inserts would accumulate stale challenge tokens).
func (d *acmednsdb) UpdateACMEDNSTXT(fqdn, value string) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain := sanitizeFQDN(fqdn)
	timenow := time.Now().Unix()
	tx, err := d.DB.Begin()
	if err != nil {
		return fmt.Errorf("failed to begin transaction: %w", err)
	}
	defer func() {
		if err != nil {
			_ = tx.Rollback()
		} else {
			_ = tx.Commit()
		}
	}()
	if _, err = tx.Exec(`INSERT INTO txt (Domain, Value, LastUpdate) VALUES (?, ?, ?)`,
		domain, value, timenow); err != nil {
		return fmt.Errorf("failed to insert txt: %w", err)
	}
	// Keep only the two newest rows for this domain.
	_, err = tx.Exec(`DELETE FROM txt WHERE Domain=? AND rowid NOT IN (
		SELECT rowid FROM txt WHERE Domain=? ORDER BY LastUpdate DESC, rowid DESC LIMIT 2)`,
		domain, domain)
	if err != nil {
		return fmt.Errorf("failed to trim txt: %w", err)
	}
	return err
}

func (d *acmednsdb) GetTXTForDomain(domain string) ([]string, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var txts []string
	getSQL := `SELECT Value FROM txt WHERE Domain=?`
	sm, err := d.DB.Prepare(getSQL)
	if err != nil {
		return txts, err
	}
	defer sm.Close()
	rows, err := sm.Query(domain)
	if err != nil {
		return txts, err
	}
	defer rows.Close()
	for rows.Next() {
		var rtxt string
		err = rows.Scan(&rtxt)
		if err != nil {
			return txts, err
		}
		txts = append(txts, rtxt)
	}
	if err := rows.Err(); err != nil {
		return txts, err
	}
	return txts, nil
}

func (d *acmednsdb) ListTXTRecords() ([]certo.TXTRecord, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var records []certo.TXTRecord
	rows, err := d.DB.Query("SELECT Domain, Value, LastUpdate FROM txt ORDER BY LastUpdate DESC")
	if err != nil {
		return records, err
	}
	defer rows.Close()
	for rows.Next() {
		var r certo.TXTRecord
		var ts int64
		err = rows.Scan(&r.Domain, &r.Value, &ts)
		if err != nil {
			return records, err
		}
		r.LastUpdate = time.Unix(ts, 0)
		records = append(records, r)
	}
	if err := rows.Err(); err != nil {
		return records, err
	}
	return records, nil
}

func (d *acmednsdb) ListTXTRecordsByDomains(domains []string) ([]certo.TXTRecord, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	if len(domains) == 0 {
		return nil, nil
	}
	var records []certo.TXTRecord
	placeholders := make([]string, len(domains))
	args := make([]interface{}, len(domains))
	for i, dom := range domains {
		placeholders[i] = "?"
		args[i] = sanitizeFQDN(dom)
	}
	query := fmt.Sprintf("SELECT Domain, Value, LastUpdate FROM txt WHERE Domain IN (%s) ORDER BY LastUpdate DESC",
		strings.Join(placeholders, ","))
	rows, err := d.DB.Query(query, args...)
	if err != nil {
		return records, err
	}
	defer rows.Close()
	for rows.Next() {
		var r certo.TXTRecord
		var ts int64
		err = rows.Scan(&r.Domain, &r.Value, &ts)
		if err != nil {
			return records, err
		}
		r.LastUpdate = time.Unix(ts, 0)
		records = append(records, r)
	}
	if err := rows.Err(); err != nil {
		return records, err
	}
	return records, nil
}

// --- User methods ---

func (d *acmednsdb) CreateUser(username, passwordHash string) (certo.User, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	now := time.Now().Unix()
	apiKey := certo.GenerateAPIKey()
	scopeJSON, _ := json.Marshal([]string{"*"})

	tx, err := d.DB.Begin()
	if err != nil {
		return certo.User{}, fmt.Errorf("failed to begin transaction: %w", err)
	}
	defer func() {
		if err != nil {
			_ = tx.Rollback()
		} else {
			_ = tx.Commit()
		}
	}()

	result, execErr := tx.Exec("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)",
		username, passwordHash, now)
	if execErr != nil {
		err = fmt.Errorf("failed to create user: %w", execErr)
		return certo.User{}, err
	}
	id, _ := result.LastInsertId()

	insKeySQL := `INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?, ?, ?, ?, ?)`
	_, execErr = tx.Exec(insKeySQL, id, "Default", apiKey, string(scopeJSON), now)
	if execErr != nil {
		err = fmt.Errorf("failed to create default key: %w", execErr)
		return certo.User{}, err
	}

	return certo.User{ID: id, Username: username, PasswordHash: passwordHash, CreatedAt: now}, nil
}

// CreateACMEDNSAccount allocates an anonymous acme-dns account in one transaction: a fresh
// "acme-<nanoid>" user (no login password), a default global API key (the acme-dns
// password — the per-subdomain owner check on /update is the real guard), and a random
// 10-char subdomain. Retries on the (astronomically unlikely) username/subdomain collision.
func (d *acmednsdb) CreateACMEDNSAccount(allowFrom []string) (string, string, string, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	now := time.Now().Unix()
	scopeJSON, _ := json.Marshal([]string{"*"})
	if allowFrom == nil {
		allowFrom = []string{}
	}
	allowFromJSON, _ := json.Marshal(allowFrom)
	// acme-dns accounts authenticate by API key only and can never log in.
	const noLoginHash = "!"
	var lastErr error
	for attempt := 0; attempt < 10; attempt++ {
		username := certo.GenerateACMEDNSUsername()
		subdomain := certo.GenerateNanoID(certo.ACMEDNSSubdomainLength)
		apiKey := certo.GenerateAPIKey()

		tx, err := d.DB.Begin()
		if err != nil {
			return "", "", "", fmt.Errorf("failed to begin transaction: %w", err)
		}
		res, execErr := tx.Exec("INSERT INTO users (username, password_hash, created_at) VALUES (?, ?, ?)",
			username, noLoginHash, now)
		if execErr != nil {
			_ = tx.Rollback()
			lastErr = execErr
			continue // username collision → retry
		}
		uid, _ := res.LastInsertId()
		if _, execErr = tx.Exec("INSERT INTO api_keys (user_id, name, key_value, scope, created_at) VALUES (?, ?, ?, ?, ?)",
			uid, "acme-dns", apiKey, string(scopeJSON), now); execErr != nil {
			_ = tx.Rollback()
			lastErr = execErr
			continue // api key collision → retry
		}
		// domain == subdomain: acme-dns accounts have no real domain, only the allocated subdomain.
		if _, execErr = tx.Exec("INSERT INTO user_domains (user_id, domain, subdomain, allowfrom) VALUES (?, ?, ?, ?)",
			uid, subdomain, subdomain, string(allowFromJSON)); execErr != nil {
			_ = tx.Rollback()
			lastErr = execErr
			continue // subdomain collision → retry
		}
		if err = tx.Commit(); err != nil {
			return "", "", "", fmt.Errorf("failed to commit acme-dns account: %w", err)
		}
		return username, apiKey, subdomain, nil
	}
	return "", "", "", fmt.Errorf("failed to create acme-dns account after retries: %w", lastErr)
}

func (d *acmednsdb) GetUserByUsername(username string) (certo.User, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var u certo.User
	getSQL := `SELECT id, username, password_hash, created_at FROM users WHERE username=?`
	err := d.DB.QueryRow(getSQL, username).Scan(&u.ID, &u.Username, &u.PasswordHash, &u.CreatedAt)
	if err != nil {
		return u, fmt.Errorf("user not found: %w", err)
	}
	return u, nil
}

func (d *acmednsdb) GetUserByID(id int64) (certo.User, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var u certo.User
	getSQL := `SELECT id, username, password_hash, created_at FROM users WHERE id=?`
	err := d.DB.QueryRow(getSQL, id).Scan(&u.ID, &u.Username, &u.PasswordHash, &u.CreatedAt)
	if err != nil {
		return u, fmt.Errorf("user not found: %w", err)
	}
	return u, nil
}

// --- Domain methods ---

func (d *acmednsdb) AddUserDomain(userID int64, username, domain string) (certo.UserDomain, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain = strings.ToLower(strings.TrimSpace(domain))
	insSQL := `INSERT INTO user_domains (user_id, domain, subdomain) VALUES (?, ?, ?)`
	// Try with increasing salt on subdomain collision (max 10 attempts)
	for salt := 0; salt < 10; salt++ {
		subdomain := certo.GenerateSubdomain(username, domain, salt)
		_, err := d.DB.Exec(insSQL, userID, domain, subdomain)
		if err == nil {
			return certo.UserDomain{Domain: domain, Subdomain: subdomain}, nil
		}
		errStr := strings.ToLower(err.Error())
		// Only retry on subdomain uniqueness collision, not on user_id+domain duplicate
		if !strings.Contains(errStr, "unique") && !strings.Contains(errStr, "duplicate") {
			return certo.UserDomain{}, fmt.Errorf("failed to add domain: %w", err)
		}
		// Check if it's a user_id+domain duplicate (not a subdomain collision)
		if strings.Contains(errStr, "user_id") || strings.Contains(errStr, "user_domains.user_id") {
			return certo.UserDomain{}, fmt.Errorf("failed to add domain: %w", err)
		}
	}
	return certo.UserDomain{}, fmt.Errorf("failed to add domain: subdomain collision after retries")
}

func (d *acmednsdb) RemoveUserDomain(userID int64, domain string) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain = strings.ToLower(strings.TrimSpace(domain))
	delSQL := `DELETE FROM user_domains WHERE user_id=? AND domain=?`
	_, err := d.DB.Exec(delSQL, userID, domain)
	return err
}

func (d *acmednsdb) GetUserDomains(userID int64) ([]certo.UserDomain, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var domains []certo.UserDomain
	getSQL := `SELECT domain, subdomain FROM user_domains WHERE user_id=? ORDER BY domain`
	rows, err := d.DB.Query(getSQL, userID)
	if err != nil {
		return domains, err
	}
	defer rows.Close()
	for rows.Next() {
		var ud certo.UserDomain
		if err := rows.Scan(&ud.Domain, &ud.Subdomain); err != nil {
			return domains, err
		}
		domains = append(domains, ud)
	}
	if err := rows.Err(); err != nil {
		return domains, err
	}
	return domains, nil
}

func (d *acmednsdb) GetSubdomainByUserDomain(userID int64, domain string) (string, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	domain = strings.ToLower(strings.TrimSpace(domain))
	var subdomain string
	getSQL := `SELECT subdomain FROM user_domains WHERE user_id=? AND domain=?`
	err := d.DB.QueryRow(getSQL, userID, domain).Scan(&subdomain)
	if err != nil {
		return "", fmt.Errorf("domain not found: %w", err)
	}
	return subdomain, nil
}

func (d *acmednsdb) GetSubdomainOwner(subdomain string) (int64, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	subdomain = strings.ToLower(strings.TrimSpace(subdomain))
	var userID int64
	getSQL := `SELECT user_id FROM user_domains WHERE subdomain=?`
	err := d.DB.QueryRow(getSQL, subdomain).Scan(&userID)
	if err != nil {
		return 0, fmt.Errorf("subdomain not found: %w", err)
	}
	return userID, nil
}

// GetUserDomainBySubdomain returns the owning user id and the real domain for a subdomain.
// Used by the acme-dns /update path, which only carries the subdomain but needs the real
// domain for the API-key scope check.
func (d *acmednsdb) GetUserDomainBySubdomain(subdomain string) (int64, string, []string, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	subdomain = strings.ToLower(strings.TrimSpace(subdomain))
	var userID int64
	var domain string
	var allowFromJSON string
	getSQL := `SELECT user_id, domain, allowfrom FROM user_domains WHERE subdomain=?`
	err := d.DB.QueryRow(getSQL, subdomain).Scan(&userID, &domain, &allowFromJSON)
	if err != nil {
		return 0, "", nil, fmt.Errorf("subdomain not found: %w", err)
	}
	var allowFrom []string
	_ = json.Unmarshal([]byte(allowFromJSON), &allowFrom)
	return userID, domain, allowFrom, nil
}

// --- Admin methods ---

func (d *acmednsdb) ListUsers() ([]certo.User, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var users []certo.User
	rows, err := d.DB.Query("SELECT id, username, password_hash, created_at FROM users ORDER BY id")
	if err != nil {
		return users, err
	}
	defer rows.Close()
	for rows.Next() {
		var u certo.User
		if err := rows.Scan(&u.ID, &u.Username, &u.PasswordHash, &u.CreatedAt); err != nil {
			return users, err
		}
		users = append(users, u)
	}
	if err := rows.Err(); err != nil {
		return users, err
	}
	return users, nil
}

func (d *acmednsdb) DeleteUser(userID int64) error {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	// Delete in FK-safe order: api_keys -> user_domains -> users
	for _, table := range []string{"api_keys", "user_domains"} {
		delSQL := fmt.Sprintf("DELETE FROM %s WHERE user_id=?", table)
		_, _ = d.DB.Exec(delSQL, userID)
	}
	delSQL := `DELETE FROM users WHERE id=?`
	_, err := d.DB.Exec(delSQL, userID)
	return err
}

func (d *acmednsdb) ListAllDomains() ([]certo.UserDomain, error) {
	d.Mutex.Lock()
	defer d.Mutex.Unlock()
	var domains []certo.UserDomain
	rows, err := d.DB.Query("SELECT ud.domain, ud.subdomain, u.username FROM user_domains ud JOIN users u ON ud.user_id = u.id ORDER BY ud.domain")
	if err != nil {
		return domains, err
	}
	defer rows.Close()
	for rows.Next() {
		var ud certo.UserDomain
		var owner string
		if err := rows.Scan(&ud.Domain, &ud.Subdomain, &owner); err != nil {
			return domains, err
		}
		ud.Owner = owner
		domains = append(domains, ud)
	}
	if err := rows.Err(); err != nil {
		return domains, err
	}
	return domains, nil
}

func (d *acmednsdb) Close() {
	d.DB.Close()
}

func (d *acmednsdb) GetBackend() *sql.DB {
	return d.DB
}

func (d *acmednsdb) SetBackend(backend *sql.DB) {
	d.DB = backend
}

func sanitizeFQDN(fqdn string) string {
	fqdn = strings.ToLower(strings.TrimSpace(fqdn))
	fqdn = strings.TrimSuffix(fqdn, ".")
	return fqdn
}
