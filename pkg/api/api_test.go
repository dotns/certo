package api

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/dotns/certo/pkg/certo"
	"github.com/dotns/certo/pkg/database"
	"github.com/dotns/certo/pkg/nameserver"

	"github.com/caddyserver/certmagic"
	"github.com/gavv/httpexpect"
	"github.com/julienschmidt/httprouter"
	"github.com/rs/cors"
	"go.uber.org/zap"
)

func fakeConfigAndLogger() (certo.Config, *zap.SugaredLogger) {
	c := certo.Config{}
	c.Database.Engine = "sqlite"
	c.Database.Connection = ":memory:"
	c.API.JWTSecret = "test-secret-key-for-testing-only"
	l := zap.NewNop().Sugar()
	return c, l
}

func getExpect(t *testing.T, server *httptest.Server) *httpexpect.Expect {
	return httpexpect.WithConfig(httpexpect.Config{
		BaseURL:  server.URL,
		Reporter: httpexpect.NewAssertReporter(t),
	})
}

func setupTestServer() (*httptest.Server, API, certo.DB) {
	router := httprouter.New()
	config, logger := fakeConfigAndLogger()
	config.General.Domain = "example.com"
	config.API.Port = "8080"
	config.API.TLS = certo.TLSProviderNone
	config.API.CorsOrigins = []string{"*"}

	db, _ := database.Init(&config, logger)
	errChan := make(chan error, 1)
	a := Init(&config, db, logger, errChan, "test")
	c := cors.New(cors.Options{
		AllowedOrigins:   config.API.CorsOrigins,
		AllowedMethods:   []string{"GET", "POST", "DELETE"},
		AllowedHeaders:   []string{"Authorization", "Content-Type"},
		AllowCredentials: true,
	})
	router.POST("/present", a.BasicAuthHTTPreq(a.webPresentPost))
	router.POST("/cleanup", a.BasicAuthHTTPreq(a.webCleanupPost))
	router.GET("/health", a.healthCheck)
	router.POST("/api/register", a.apiRegister)
	router.POST("/api/login", a.apiLogin)
	router.GET("/api/info", a.apiInfo)
	router.GET("/api/profile", a.JWTOrGlobalKeyAuth(a.apiProfile))
	router.GET("/api/keys", a.JWTOrGlobalKeyAuth(a.apiListKeys))
	router.GET("/api/domains", a.JWTOrKeyAuth(a.apiGetDomains))
	router.POST("/api/domains", a.JWTOrKeyAuth(a.apiAddDomain))
	router.DELETE("/api/domains/:domain", a.JWTOrKeyAuth(a.apiRemoveDomain))
	router.GET("/api/records", a.JWTOrKeyAuth(a.apiGetRecords))
	router.POST("/api/keys", a.JWTOrGlobalKeyAuth(a.apiCreateKey))
	router.POST("/register", a.acmednsRegister)
	router.POST("/update", a.acmednsUpdate)
	router.GET("/acmedns", a.acmednsStorageFetchAll)
	router.GET("/acmedns/:domain", a.acmednsStorageFetch)
	router.POST("/acmedns/:domain", a.acmednsStoragePut)

	server := httptest.NewServer(c.Handler(router))
	return server, a, db
}

func registerAndLogin(e *httpexpect.Expect, username, password string) (token, apiKey string) {
	resp := e.POST("/api/register").
		WithJSON(map[string]string{"username": username, "password": password}).
		Expect().Status(http.StatusCreated).JSON().Object()
	token = resp.Value("token").String().Raw()
	// Get Default API key from /api/keys
	keys := e.GET("/api/keys").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).JSON().Array()
	apiKey = keys.Element(0).Object().Value("key").String().Raw()
	return
}

func TestApiRegisterAndLogin(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	// Register
	e.POST("/api/register").
		WithJSON(map[string]string{"username": "alice", "password": "secret123"}).
		Expect().Status(http.StatusCreated).
		JSON().Object().
		ContainsKey("token").
		ContainsKey("username")

	// Duplicate register
	e.POST("/api/register").
		WithJSON(map[string]string{"username": "alice", "password": "otherpass123"}).
		Expect().Status(http.StatusConflict)

	// Login
	e.POST("/api/login").
		WithJSON(map[string]string{"username": "alice", "password": "secret123"}).
		Expect().Status(http.StatusOK).
		JSON().Object().ContainsKey("token")

	// Wrong password
	e.POST("/api/login").
		WithJSON(map[string]string{"username": "alice", "password": "wrong"}).
		Expect().Status(http.StatusUnauthorized)

	// Short password
	e.POST("/api/register").
		WithJSON(map[string]string{"username": "bob", "password": "short"}).
		Expect().Status(http.StatusBadRequest)
}

func TestApiDomainManagement(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, _ := registerAndLogin(e, "domuser", "password123")

	// No auth
	e.GET("/api/domains").Expect().Status(http.StatusUnauthorized)

	// Add domain
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]string{"domain": "example.com"}).
		Expect().Status(http.StatusCreated).
		JSON().Object().
		ValueEqual("domain", "example.com").
		ContainsKey("cname_target")

	// List domains
	domains := e.GET("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).
		JSON().Array()
	domains.Length().Equal(1)
	domains.Element(0).Object().ValueEqual("domain", "example.com")

	// Duplicate domain
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]string{"domain": "example.com"}).
		Expect().Status(http.StatusConflict)

	// Delete domain
	e.DELETE("/api/domains/example.com").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusNoContent)

	// Verify empty
	e.GET("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).
		JSON().Array().Length().Equal(0)
}

func TestApiDomainIsolation(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token1, _ := registerAndLogin(e, "user1", "password1")
	token2, _ := registerAndLogin(e, "user2", "password2")

	// user1 adds domain
	resp1 := e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token1).
		WithJSON(map[string]string{"domain": "shared.com"}).
		Expect().Status(http.StatusCreated).JSON().Object()
	sub1 := resp1.Value("subdomain").String().Raw()

	// user2 can also add the same domain — gets a different nanoid subdomain
	resp2 := e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token2).
		WithJSON(map[string]string{"domain": "shared.com"}).
		Expect().Status(http.StatusCreated).JSON().Object()
	sub2 := resp2.Value("subdomain").String().Raw()

	if sub1 == sub2 {
		t.Error("Different users should get different subdomains for same domain")
	}

	// But same user cannot add the same domain twice
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token1).
		WithJSON(map[string]string{"domain": "shared.com"}).
		Expect().Status(http.StatusConflict)
}

func TestApiPresentWithDBAuth(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	// Register user and add domain via API
	token, apiKey := registerAndLogin(e, "dnsuser", "password123")
	resp := e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]string{"domain": "example.com"}).
		Expect().Status(http.StatusCreated).JSON().Object()
	subdomain := resp.Value("subdomain").String().Raw()

	// Present with Basic Auth (matching DB user)
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.example.com.", "value": "token123"}).
		WithBasicAuth("dnsuser", apiKey).
		Expect().Status(http.StatusOK)

	// Verify TXT in DB — stored under <nanoid>.example.com
	internalDomain := subdomain + ".example.com"
	txts, _ := db.GetTXTForDomain(internalDomain)
	if len(txts) != 1 || txts[0] != "token123" {
		t.Errorf("Expected TXT record at %s, got %v", internalDomain, txts)
	}

	// Present for a not-yet-added domain: the default (global) key carries creation
	// rights, so the domain is auto-provisioned and the record is stored.
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.other.com.", "value": "token456"}).
		WithBasicAuth("dnsuser", apiKey).
		Expect().Status(http.StatusOK)

	// Cleanup
	e.POST("/cleanup").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.example.com.", "value": "token123"}).
		WithBasicAuth("dnsuser", apiKey).
		Expect().Status(http.StatusOK)
}

func TestApiRecords(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, _ := registerAndLogin(e, "recuser", "password123")
	resp := e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]string{"domain": "rec.com"}).
		Expect().Status(http.StatusCreated).JSON().Object()
	subdomain := resp.Value("subdomain").String().Raw()

	// Insert a TXT record using the nanoid-based internal domain
	internalDomain := subdomain + ".example.com"
	_ = db.PresentTXT(internalDomain, "testval")

	// Get records
	records := e.GET("/api/records").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).
		JSON().Array()
	records.Length().Equal(1)
	records.Element(0).Object().ValueEqual("value", "testval")
}

func TestApiInfo(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	e.GET("/api/info").Expect().Status(http.StatusOK).
		JSON().Object().ValueEqual("base_domain", "example.com")
}

func TestApiHealthCheck(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)
	e.GET("/health").Expect().Status(http.StatusOK)
}

func TestSetupTLS(t *testing.T) {
	server, svr, _ := setupTestServer()
	defer server.Close()

	for _, test := range []struct {
		apiTls     string
		expectedCA string
	}{
		{certo.TLSProviderLetsEncrypt, certmagic.LetsEncryptProductionCA},
		{certo.TLSProviderLetsEncryptStaging, certmagic.LetsEncryptStagingCA},
	} {
		svr.Config.API.TLS = test.apiTls
		ns := &nameserver.Nameserver{}
		magic := svr.setupTLS([]certo.NS{ns})
		if test.expectedCA != certmagic.DefaultACME.CA {
			t.Errorf("got CA %s, want %s", certmagic.DefaultACME.CA, test.expectedCA)
		}
		if magic.DefaultServerName != svr.Config.General.Domain {
			t.Errorf("got domain %s, want %s", magic.DefaultServerName, svr.Config.General.Domain)
		}
	}
}
