package api

import (
	"net/http"
	"strings"
	"testing"
)

// a valid acme-dns txt value is exactly 43 chars
func token43(c string) string { return strings.Repeat(c, 43) }

func contains(ss []string, want string) bool {
	for _, s := range ss {
		if s == want {
			return true
		}
	}
	return false
}

func TestACMEDNSStorageFetchProvisions(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, apiKey := registerAndLogin(e, "adfetch", "password123")

	// Fetch storage for a domain the user has NOT added yet: it is auto-provisioned.
	acct := e.GET("/acmedns/example.com").
		WithBasicAuth("adfetch", apiKey).
		Expect().Status(http.StatusOK).JSON().Object()
	acct.ValueEqual("username", "adfetch")
	acct.ValueEqual("password", apiKey)
	sub := acct.Value("subdomain").String().Raw()
	acct.ValueEqual("fulldomain", sub+".example.com")
	acct.Value("server_url").String().NotContains("/acmedns")

	// The domain now shows up on the account side (auto-provisioned).
	domains := e.GET("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).JSON().Array()
	domains.Length().Equal(1)
	domains.Element(0).Object().ValueEqual("domain", "example.com").ValueEqual("subdomain", sub)

	// Idempotent: fetching again returns the same (deterministic) subdomain.
	e.GET("/acmedns/example.com").
		WithBasicAuth("adfetch", apiKey).
		Expect().Status(http.StatusOK).JSON().Object().ValueEqual("subdomain", sub)

	// Wildcard cert domain normalizes to the same record.
	e.GET("/acmedns/%2A.example.com").
		WithBasicAuth("adfetch", apiKey).
		Expect().Status(http.StatusOK).JSON().Object().ValueEqual("subdomain", sub)
}

func TestACMEDNSRegisterAndUpdate(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	// Anonymous registration → fresh acme-<nanoid> account + random 10-char subdomain.
	acct := e.POST("/register").
		WithJSON(map[string]interface{}{"allowfrom": []string{}}).
		Expect().Status(http.StatusCreated).JSON().Object()
	username := acct.Value("username").String().Raw()
	password := acct.Value("password").String().Raw()
	sub := acct.Value("subdomain").String().Raw()
	if !strings.HasPrefix(username, "acme-") {
		t.Errorf("expected acme- username, got %q", username)
	}
	if len(sub) != 10 {
		t.Errorf("expected 10-char nanoid subdomain, got %q (len %d)", sub, len(sub))
	}
	acct.ValueEqual("fulldomain", sub+".example.com")

	// The returned credentials drive the native /update (stock acme-dns flow).
	e.POST("/update").
		WithHeader("X-Api-User", username).
		WithHeader("X-Api-Key", password).
		WithJSON(map[string]string{"subdomain": sub, "txt": token43("z")}).
		Expect().Status(http.StatusOK).JSON().Object().ValueEqual("txt", token43("z"))

	txts, _ := db.GetTXTForDomain(sub + ".example.com")
	if !contains(txts, token43("z")) {
		t.Errorf("expected TXT at %s.example.com, got %v", sub, txts)
	}

	// Two registrations get different accounts/subdomains.
	acct2 := e.POST("/register").Expect().Status(http.StatusCreated).JSON().Object()
	if acct2.Value("subdomain").String().Raw() == sub {
		t.Error("expected distinct subdomains across registrations")
	}
}

func TestACMEDNSAllowFrom(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	// allowfrom that excludes the test client (loopback) → /update rejected.
	deny := e.POST("/register").
		WithJSON(map[string]interface{}{"allowfrom": []string{"10.0.0.0/8"}}).
		Expect().Status(http.StatusCreated).JSON().Object()
	e.POST("/update").
		WithHeader("X-Api-User", deny.Value("username").String().Raw()).
		WithHeader("X-Api-Key", deny.Value("password").String().Raw()).
		WithJSON(map[string]string{"subdomain": deny.Value("subdomain").String().Raw(), "txt": token43("a")}).
		Expect().Status(http.StatusForbidden)

	// allowfrom that includes loopback → /update allowed.
	allow := e.POST("/register").
		WithJSON(map[string]interface{}{"allowfrom": []string{"127.0.0.0/8", "::1/128"}}).
		Expect().Status(http.StatusCreated).JSON().Object()
	e.POST("/update").
		WithHeader("X-Api-User", allow.Value("username").String().Raw()).
		WithHeader("X-Api-Key", allow.Value("password").String().Raw()).
		WithJSON(map[string]string{"subdomain": allow.Value("subdomain").String().Raw(), "txt": token43("a")}).
		Expect().Status(http.StatusOK)
}

func TestACMEDNSStorageFetchAuth(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	_, apiKey := registerAndLogin(e, "adauth", "password123")

	// No credentials.
	e.GET("/acmedns/example.com").Expect().Status(http.StatusUnauthorized)
	// Wrong key.
	e.GET("/acmedns/example.com").
		WithBasicAuth("adauth", "wrongkey").
		Expect().Status(http.StatusUnauthorized)
	// Right key, wrong user.
	e.GET("/acmedns/example.com").
		WithBasicAuth("nobody", apiKey).
		Expect().Status(http.StatusUnauthorized)
}

func TestACMEDNSUpdateAndCrossProtocol(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	_, apiKey := registerAndLogin(e, "adupd", "password123")

	// Provision via storage fetch.
	sub := e.GET("/acmedns/cross.com").
		WithBasicAuth("adupd", apiKey).
		Expect().Status(http.StatusOK).JSON().Object().Value("subdomain").String().Raw()
	internal := sub + ".example.com"

	// acme-dns /update writes the TXT.
	adToken := token43("a")
	e.POST("/update").
		WithHeader("X-Api-User", "adupd").
		WithHeader("X-Api-Key", apiKey).
		WithJSON(map[string]string{"subdomain": sub, "txt": adToken}).
		Expect().Status(http.StatusOK).JSON().Object().ValueEqual("txt", adToken)

	// httpreq /present writes to the SAME record (different protocol, same subdomain).
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.cross.com.", "value": "httpreq-val"}).
		WithBasicAuth("adupd", apiKey).
		Expect().Status(http.StatusOK)

	txts, _ := db.GetTXTForDomain(internal)
	if !contains(txts, adToken) || !contains(txts, "httpreq-val") {
		t.Errorf("expected both acme-dns and httpreq values at %s, got %v", internal, txts)
	}
}

func TestACMEDNSUpdateValidation(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	_, apiKey := registerAndLogin(e, "adval", "password123")
	sub := e.GET("/acmedns/val.com").
		WithBasicAuth("adval", apiKey).
		Expect().Status(http.StatusOK).JSON().Object().Value("subdomain").String().Raw()

	// txt not exactly 43 chars.
	e.POST("/update").
		WithHeader("X-Api-User", "adval").WithHeader("X-Api-Key", apiKey).
		WithJSON(map[string]string{"subdomain": sub, "txt": "tooshort"}).
		Expect().Status(http.StatusBadRequest)

	// Unknown subdomain → forbidden.
	e.POST("/update").
		WithHeader("X-Api-User", "adval").WithHeader("X-Api-Key", apiKey).
		WithJSON(map[string]string{"subdomain": "deadbeef", "txt": token43("a")}).
		Expect().Status(http.StatusForbidden)

	// Bad credentials.
	e.POST("/update").
		WithHeader("X-Api-User", "adval").WithHeader("X-Api-Key", "wrong").
		WithJSON(map[string]string{"subdomain": sub, "txt": token43("a")}).
		Expect().Status(http.StatusUnauthorized)

	// Another user cannot update this subdomain.
	_, otherKey := registerAndLogin(e, "adval2", "password123")
	e.POST("/update").
		WithHeader("X-Api-User", "adval2").WithHeader("X-Api-Key", otherKey).
		WithJSON(map[string]string{"subdomain": sub, "txt": token43("a")}).
		Expect().Status(http.StatusForbidden)
}

func TestACMEDNSRollingCap(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	_, apiKey := registerAndLogin(e, "adroll", "password123")
	sub := e.GET("/acmedns/roll.com").
		WithBasicAuth("adroll", apiKey).
		Expect().Status(http.StatusOK).JSON().Object().Value("subdomain").String().Raw()

	for _, c := range []string{"1", "2", "3"} {
		e.POST("/update").
			WithHeader("X-Api-User", "adroll").WithHeader("X-Api-Key", apiKey).
			WithJSON(map[string]string{"subdomain": sub, "txt": token43(c)}).
			Expect().Status(http.StatusOK)
	}

	txts, _ := db.GetTXTForDomain(sub + ".example.com")
	if len(txts) != 2 {
		t.Fatalf("expected exactly 2 rolling TXT values, got %d: %v", len(txts), txts)
	}
	if contains(txts, token43("1")) {
		t.Errorf("oldest value should have been trimmed, got %v", txts)
	}
	if !contains(txts, token43("2")) || !contains(txts, token43("3")) {
		t.Errorf("expected the two newest values, got %v", txts)
	}
}

func TestACMEDNSScopedKey(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, _ := registerAndLogin(e, "adscope", "password123")

	// Create a scoped key limited to *.allowed.com.
	scopedKey := e.POST("/api/keys").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]interface{}{"name": "scoped", "scope": []string{"*.allowed.com"}}).
		Expect().Status(http.StatusCreated).JSON().Object().Value("key").String().Raw()

	// In-scope domain → provisioned.
	e.GET("/acmedns/allowed.com").
		WithBasicAuth("adscope", scopedKey).
		Expect().Status(http.StatusOK)

	// Out-of-scope domain → forbidden.
	e.GET("/acmedns/denied.com").
		WithBasicAuth("adscope", scopedKey).
		Expect().Status(http.StatusForbidden)

	// Provision an out-of-scope domain via the global token, then confirm FetchAll with
	// the scoped key only returns in-scope domains (no information disclosure).
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]string{"domain": "other.com"}).
		Expect().Status(http.StatusCreated)

	all := e.GET("/acmedns").
		WithBasicAuth("adscope", scopedKey).
		Expect().Status(http.StatusOK).JSON().Object()
	all.ContainsKey("allowed.com")
	all.NotContainsKey("other.com")
}

func TestPresentWildcardKeyAutoCreates(t *testing.T) {
	server, _, db := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	// The default key is global ("*") → carries creation rights, so /present for a
	// domain the user never added provisions it instead of returning 403.
	token, apiKey := registerAndLogin(e, "autocreate", "password123")
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.fresh.com.", "value": "tok"}).
		WithBasicAuth("autocreate", apiKey).
		Expect().Status(http.StatusOK)

	// It now exists on the account side.
	domains := e.GET("/api/domains").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).JSON().Array()
	domains.Length().Equal(1)
	sub := domains.Element(0).Object().ValueEqual("domain", "fresh.com").Value("subdomain").String().Raw()

	// And the TXT landed under the new subdomain.
	txts, _ := db.GetTXTForDomain(sub + ".example.com")
	if !contains(txts, "tok") {
		t.Errorf("expected TXT at %s.example.com, got %v", sub, txts)
	}
}

func TestPresentExactScopeKeyCreatesOwnDomain(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, _ := registerAndLogin(e, "exact", "password123")
	// An exact-domain scope can create/manage the domain(s) it covers, but nothing else.
	exactKey := e.POST("/api/keys").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]interface{}{"name": "exact", "scope": []string{"specific.com"}}).
		Expect().Status(http.StatusCreated).JSON().Object().Value("key").String().Raw()

	// Its own in-scope domain → auto-created on present.
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.specific.com.", "value": "tok"}).
		WithBasicAuth("exact", exactKey).
		Expect().Status(http.StatusOK)

	// A domain outside its scope → 403, no creation.
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.other.com.", "value": "tok"}).
		WithBasicAuth("exact", exactKey).
		Expect().Status(http.StatusForbidden)
}

func TestAddDomainScopeEnforced(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	token, _ := registerAndLogin(e, "scopeadd", "password123")
	scoped := e.POST("/api/keys").
		WithHeader("Authorization", "Bearer "+token).
		WithJSON(map[string]interface{}{"name": "limited", "scope": []string{"*.mine.test"}}).
		Expect().Status(http.StatusCreated).JSON().Object()
	key := scoped.Value("key").String().Raw()

	// Out-of-scope add is rejected (no scope-escalation via /api/domains).
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+key).
		WithJSON(map[string]string{"domain": "evil.test"}).
		Expect().Status(http.StatusForbidden)

	// In-scope add is allowed.
	e.POST("/api/domains").
		WithHeader("Authorization", "Bearer "+key).
		WithJSON(map[string]string{"domain": "a.mine.test"}).
		Expect().Status(http.StatusCreated)

	// The scoped key's scope must NOT have been auto-expanded by the add.
	keys := e.GET("/api/keys").
		WithHeader("Authorization", "Bearer "+token).
		Expect().Status(http.StatusOK).JSON().Array()
	for i := 0; i < int(keys.Length().Raw()); i++ {
		k := keys.Element(i).Object()
		if k.Value("name").String().Raw() == "limited" {
			k.Value("scope").Array().Equal([]string{"*.mine.test"})
		}
	}
}

func TestPresentValueTooLong(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	_, apiKey := registerAndLogin(e, "toolong", "password123")
	e.POST("/present").
		WithJSON(map[string]string{"fqdn": "_acme-challenge.big.com.", "value": strings.Repeat("x", 256)}).
		WithBasicAuth("toolong", apiKey).
		Expect().Status(http.StatusBadRequest)
}

func TestRegistrationDisabled(t *testing.T) {
	server, a, _ := setupTestServer()
	defer server.Close()
	disabled := false
	a.Config.API.AllowRegistration = &disabled // shared *Config; handlers observe it
	e := getExpect(t, server)

	// Dashboard registration blocked.
	e.POST("/api/register").
		WithJSON(map[string]string{"username": "nope", "password": "password123"}).
		Expect().Status(http.StatusForbidden)
	// acme-dns anonymous registration blocked.
	e.POST("/register").Expect().Status(http.StatusForbidden)
}

func TestACMEDNSInfoCapability(t *testing.T) {
	server, _, _ := setupTestServer()
	defer server.Close()
	e := getExpect(t, server)

	caps := e.GET("/api/info").Expect().Status(http.StatusOK).
		JSON().Object().Value("capabilities").Array()
	caps.Contains("acmedns")
}
