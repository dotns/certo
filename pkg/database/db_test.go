package database

import (
	"database/sql"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"go.uber.org/zap"

	"github.com/dotns/certo/pkg/certo"
)

func fakeConfigAndLogger() (certo.Config, *zap.SugaredLogger) {
	c := certo.Config{}
	c.Database.Engine = "sqlite"
	c.Database.Connection = ":memory:"
	l := zap.NewNop().Sugar()
	return c, l
}

func fakeDB() certo.DB {
	conf, logger := fakeConfigAndLogger()
	db, _ := Init(&conf, logger)
	return db
}

func TestInitCreatesCurrentSchema(t *testing.T) {
	db := fakeDB()
	defer db.Close()

	var version string
	if err := db.GetBackend().QueryRow("SELECT Value FROM acmedns WHERE Name='db_version'").Scan(&version); err != nil {
		t.Fatalf("db_version missing: %v", err)
	}
	if version != strconv.Itoa(DBVersion) {
		t.Fatalf("Expected db_version %d, got %s", DBVersion, version)
	}

	rows, err := db.GetBackend().Query("PRAGMA table_info(users)")
	if err != nil {
		t.Fatalf("PRAGMA table_info(users) failed: %v", err)
	}
	defer rows.Close()
	for rows.Next() {
		var cid int
		var name, ctype string
		var notnull int
		var dfltValue *string
		var pk int
		if err := rows.Scan(&cid, &name, &ctype, &notnull, &dfltValue, &pk); err != nil {
			t.Fatalf("Scan table info failed: %v", err)
		}
		if name == "api_key" {
			t.Fatal("users.api_key should not exist in the current schema")
		}
	}
}

func TestMigrateV1ToV2(t *testing.T) {
	path := filepath.Join(t.TempDir(), "v1.db")
	raw, err := sql.Open("sqlite", path)
	if err != nil {
		t.Fatalf("Open sqlite failed: %v", err)
	}
	// Recreate the v1 schema: user_domains WITHOUT the allowfrom column, db_version=1.
	stmts := []string{
		"CREATE TABLE acmedns(Name TEXT, Value TEXT)",
		"INSERT INTO acmedns (Name, Value) VALUES ('db_version', '1')",
		"CREATE TABLE users(id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL, created_at INT NOT NULL)",
		"CREATE TABLE user_domains(user_id INTEGER NOT NULL, domain TEXT NOT NULL, subdomain TEXT UNIQUE NOT NULL, UNIQUE(user_id, domain))",
	}
	for _, s := range stmts {
		if _, err := raw.Exec(s); err != nil {
			t.Fatalf("seed v1 schema (%q): %v", s, err)
		}
	}
	if err := raw.Close(); err != nil {
		t.Fatalf("Close raw DB failed: %v", err)
	}

	conf, logger := fakeConfigAndLogger()
	conf.Database.Connection = path
	db, err := Init(&conf, logger)
	if err != nil {
		t.Fatalf("Init (migrate v1->v2) failed: %v", err)
	}
	defer db.Close()

	var version string
	if err := db.GetBackend().QueryRow("SELECT Value FROM acmedns WHERE Name='db_version'").Scan(&version); err != nil {
		t.Fatalf("db_version missing after migrate: %v", err)
	}
	if version != "2" {
		t.Fatalf("Expected db_version 2 after migration, got %s", version)
	}

	rows, err := db.GetBackend().Query("PRAGMA table_info(user_domains)")
	if err != nil {
		t.Fatalf("PRAGMA table_info(user_domains) failed: %v", err)
	}
	defer rows.Close()
	hasAllowFrom := false
	for rows.Next() {
		var cid, notnull, pk int
		var name, ctype string
		var dflt *string
		if err := rows.Scan(&cid, &name, &ctype, &notnull, &dflt, &pk); err != nil {
			t.Fatalf("Scan table info failed: %v", err)
		}
		if name == "allowfrom" {
			hasAllowFrom = true
		}
	}
	if !hasAllowFrom {
		t.Fatal("expected user_domains.allowfrom column after migration")
	}
}

func TestInitRejectsUnsupportedDBVersion(t *testing.T) {
	path := filepath.Join(t.TempDir(), "unsupported.db")
	rawDB, err := sql.Open("sqlite", path)
	if err != nil {
		t.Fatalf("Open sqlite failed: %v", err)
	}
	if _, err := rawDB.Exec("CREATE TABLE acmedns(Name TEXT, Value TEXT)"); err != nil {
		t.Fatalf("Create acmedns failed: %v", err)
	}
	if _, err := rawDB.Exec("INSERT INTO acmedns (Name, Value) VALUES ('db_version', '6')"); err != nil {
		t.Fatalf("Insert db_version failed: %v", err)
	}
	if err := rawDB.Close(); err != nil {
		t.Fatalf("Close raw DB failed: %v", err)
	}

	conf, logger := fakeConfigAndLogger()
	conf.Database.Connection = path
	db, err := Init(&conf, logger)
	if err == nil {
		db.Close()
		t.Fatal("Expected unsupported database version to be rejected")
	}
	if !strings.Contains(err.Error(), "regenerate the database") {
		t.Fatalf("Expected regenerate error, got %v", err)
	}
}

func TestPresentTXT(t *testing.T) {
	db := fakeDB()
	err := db.PresentTXT("_acme-challenge.example.com.", "test-value-1")
	if err != nil {
		t.Fatalf("PresentTXT failed: %v", err)
	}

	txts, err := db.GetTXTForDomain("_acme-challenge.example.com")
	if err != nil {
		t.Fatalf("GetTXTForDomain failed: %v", err)
	}
	if len(txts) != 1 {
		t.Fatalf("Expected 1 TXT record, got %d", len(txts))
	}
	if txts[0] != "test-value-1" {
		t.Errorf("Expected value %q, got %q", "test-value-1", txts[0])
	}
}

func TestPresentMultipleTXT(t *testing.T) {
	db := fakeDB()
	_ = db.PresentTXT("_acme-challenge.example.com.", "value1")
	_ = db.PresentTXT("_acme-challenge.example.com.", "value2")

	txts, err := db.GetTXTForDomain("_acme-challenge.example.com")
	if err != nil {
		t.Fatalf("GetTXTForDomain failed: %v", err)
	}
	if len(txts) != 2 {
		t.Fatalf("Expected 2 TXT records, got %d", len(txts))
	}
}

func TestCleanupTXT(t *testing.T) {
	db := fakeDB()
	fqdn := "_acme-challenge.example.com."
	value := "cleanup-test-value"

	_ = db.PresentTXT(fqdn, value)

	err := db.CleanupTXT(fqdn, value)
	if err != nil {
		t.Fatalf("CleanupTXT failed: %v", err)
	}

	txts, err := db.GetTXTForDomain("_acme-challenge.example.com")
	if err != nil {
		t.Fatalf("GetTXTForDomain after cleanup failed: %v", err)
	}
	if len(txts) != 0 {
		t.Errorf("Expected 0 TXT records after cleanup, got %d", len(txts))
	}
}

func TestCleanupOnlyMatchingValue(t *testing.T) {
	db := fakeDB()
	fqdn := "_acme-challenge.example.com."

	_ = db.PresentTXT(fqdn, "value1")
	_ = db.PresentTXT(fqdn, "value2")

	// Cleanup only value1
	_ = db.CleanupTXT(fqdn, "value1")

	txts, _ := db.GetTXTForDomain("_acme-challenge.example.com")
	if len(txts) != 1 {
		t.Fatalf("Expected 1 TXT record after partial cleanup, got %d", len(txts))
	}
	if txts[0] != "value2" {
		t.Errorf("Expected remaining value %q, got %q", "value2", txts[0])
	}
}

func TestGetTXTForDomainNotFound(t *testing.T) {
	db := fakeDB()
	txts, err := db.GetTXTForDomain("does-not-exist")
	if err != nil {
		t.Fatalf("Unexpected error: %v", err)
	}
	if len(txts) != 0 {
		t.Errorf("Expected 0 records for non-existent domain, got %d", len(txts))
	}
}

func TestListTXTRecords(t *testing.T) {
	db := fakeDB()

	// Empty initially
	records, err := db.ListTXTRecords()
	if err != nil {
		t.Fatalf("ListTXTRecords failed: %v", err)
	}
	if len(records) != 0 {
		t.Errorf("Expected 0 records initially, got %d", len(records))
	}

	// Add some records
	_ = db.PresentTXT("_acme-challenge.a.com.", "val-a")
	_ = db.PresentTXT("_acme-challenge.b.com.", "val-b")

	records, err = db.ListTXTRecords()
	if err != nil {
		t.Fatalf("ListTXTRecords failed: %v", err)
	}
	if len(records) != 2 {
		t.Errorf("Expected 2 records, got %d", len(records))
	}
}

func TestFQDNNormalization(t *testing.T) {
	db := fakeDB()

	// Present with trailing dot
	_ = db.PresentTXT("_acme-challenge.EXAMPLE.COM.", "token")

	// Should be found without trailing dot, lowercase
	txts, _ := db.GetTXTForDomain("_acme-challenge.example.com")
	if len(txts) != 1 {
		t.Fatalf("Expected 1 TXT record after normalization, got %d", len(txts))
	}
}

func TestCreateAndGetUser(t *testing.T) {
	db := fakeDB()
	user, err := db.CreateUser("testuser", "$2a$10$somehash")
	if err != nil {
		t.Fatalf("CreateUser failed: %v", err)
	}
	if user.Username != "testuser" {
		t.Errorf("Expected username testuser, got %s", user.Username)
	}
	if user.ID == 0 {
		t.Error("Expected non-zero user ID")
	}

	got, err := db.GetUserByUsername("testuser")
	if err != nil {
		t.Fatalf("GetUserByUsername failed: %v", err)
	}
	if got.ID != user.ID {
		t.Errorf("Expected ID %d, got %d", user.ID, got.ID)
	}

	got2, err := db.GetUserByID(user.ID)
	if err != nil {
		t.Fatalf("GetUserByID failed: %v", err)
	}
	if got2.Username != "testuser" {
		t.Errorf("Expected username testuser, got %s", got2.Username)
	}
}

func TestDuplicateUser(t *testing.T) {
	db := fakeDB()
	_, _ = db.CreateUser("dup", "$2a$10$hash")
	_, err := db.CreateUser("dup", "$2a$10$hash2")
	if err == nil {
		t.Error("Expected error creating duplicate user")
	}
}

func TestUserDomains(t *testing.T) {
	db := fakeDB()
	user, _ := db.CreateUser("domuser", "$2a$10$hash")

	// Add domains — returns UserDomain with nanoid subdomain
	ud1, err := db.AddUserDomain(user.ID, "domuser", "example.com")
	if err != nil {
		t.Fatalf("AddUserDomain failed: %v", err)
	}
	if ud1.Subdomain == "" {
		t.Fatal("Expected non-empty subdomain")
	}
	if ud1.Domain != "example.com" {
		t.Errorf("Expected domain example.com, got %s", ud1.Domain)
	}

	ud2, err := db.AddUserDomain(user.ID, "domuser", "other.com")
	if err != nil {
		t.Fatalf("AddUserDomain failed: %v", err)
	}
	if ud1.Subdomain == ud2.Subdomain {
		t.Error("Expected different subdomains for different domains")
	}

	// List domains
	domains, err := db.GetUserDomains(user.ID)
	if err != nil {
		t.Fatalf("GetUserDomains failed: %v", err)
	}
	if len(domains) != 2 {
		t.Fatalf("Expected 2 domains, got %d", len(domains))
	}

	// Look up subdomain by user+domain
	sub, err := db.GetSubdomainByUserDomain(user.ID, "example.com")
	if err != nil {
		t.Fatalf("GetSubdomainByUserDomain failed: %v", err)
	}
	if sub != ud1.Subdomain {
		t.Errorf("Expected subdomain %s, got %s", ud1.Subdomain, sub)
	}

	// Different user can add same domain (gets different nanoid)
	user2, _ := db.CreateUser("domuser2", "$2a$10$hash2")
	ud3, err := db.AddUserDomain(user2.ID, "domuser2", "example.com")
	if err != nil {
		t.Fatalf("Second user AddUserDomain failed: %v", err)
	}
	if ud3.Subdomain == ud1.Subdomain {
		t.Error("Different users should get different subdomains for same domain")
	}

	// Remove domain
	if err := db.RemoveUserDomain(user.ID, "example.com"); err != nil {
		t.Fatalf("RemoveUserDomain failed: %v", err)
	}
	domains, _ = db.GetUserDomains(user.ID)
	if len(domains) != 1 {
		t.Errorf("Expected 1 domain after removal, got %d", len(domains))
	}
}

func TestListTXTRecordsByDomains(t *testing.T) {
	db := fakeDB()
	_ = db.PresentTXT("a.com.example.com", "val-a")
	_ = db.PresentTXT("b.com.example.com", "val-b")
	_ = db.PresentTXT("c.com.example.com", "val-c")

	records, err := db.ListTXTRecordsByDomains([]string{"a.com.example.com", "b.com.example.com"})
	if err != nil {
		t.Fatalf("ListTXTRecordsByDomains failed: %v", err)
	}
	if len(records) != 2 {
		t.Errorf("Expected 2 records, got %d", len(records))
	}

	// Empty list
	records, _ = db.ListTXTRecordsByDomains(nil)
	if len(records) != 0 {
		t.Errorf("Expected 0 records for nil domains, got %d", len(records))
	}
}
