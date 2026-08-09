package api

import (
	"encoding/json"
	"net"
	"net/http"
	"net/url"
	"strings"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
)

// acme-dns protocol support.
//
// certo implements lego's acme-dns provider (github.com/go-acme/lego) over the *same*
// records as the httpreq endpoints. The native protocol sits on the root paths /register
// and /update (ACME_DNS_API_BASE), so any stock acme-dns client with local file storage
// works. certo additionally acts as lego's HTTP storage backend under /acmedns
// (ACME_DNS_STORAGE_BASE_URL), so an existing certo user drives acme-dns with zero local
// config: lego fetches the account (certo username + API key + subdomain) from certo and
// then calls /update.

// authUserKey validates a (username, api_key) pair the same way BasicAuthHTTPreq does and
// returns the user and key. It is the shared auth primitive for the acme-dns endpoints.
func (a *API) authUserKey(username, secret string) (certo.User, certo.APIKey, bool) {
	if username == "" || secret == "" {
		return certo.User{}, certo.APIKey{}, false
	}
	user, err := a.DB.GetUserByUsername(username)
	if err != nil {
		return certo.User{}, certo.APIKey{}, false
	}
	apiKey, err := a.DB.GetAPIKeyByValue(secret)
	if err != nil || apiKey.UserID != user.ID {
		return certo.User{}, certo.APIKey{}, false
	}
	return user, apiKey, true
}

// normalizeACMEDNSDomain maps a lego storage key (the cert domain) onto a certo domain:
// URL-unescapes (the router may hand back an escaped "%2A" wildcard), lowercases, trims,
// and drops a leading "*." wildcard label and trailing dot, so "*.example.com." and
// "example.com" resolve to the same record.
func normalizeACMEDNSDomain(domain string) string {
	if dec, err := url.PathUnescape(domain); err == nil {
		domain = dec
	}
	domain = strings.ToLower(strings.TrimSpace(domain))
	domain = strings.TrimSuffix(domain, ".")
	domain = strings.TrimPrefix(domain, "*.")
	return domain
}

// requestScheme best-effort determines the external scheme for building server_url.
func requestScheme(r *http.Request) string {
	if proto := r.Header.Get("X-Forwarded-Proto"); proto != "" {
		return proto
	}
	if r.TLS != nil {
		return "https"
	}
	return "http"
}

// buildAccount assembles the acme-dns storage account for a certo subdomain. The password
// is the API key the caller already authenticated with (echoed back, not looked up).
// ServerURL is the native acme-dns API base (root) used for /update — NOT the storage path.
func (a *API) buildAccount(r *http.Request, username, apiKey, subdomain string) certo.ACMEDNSAccount {
	base := a.Config.General.Domain
	return certo.ACMEDNSAccount{
		FullDomain: certo.InternalDomain(subdomain, base),
		SubDomain:  subdomain,
		Username:   username,
		Password:   apiKey,
		ServerURL:  requestScheme(r) + "://" + r.Host,
	}
}

// getOrCreateSubdomain returns the subdomain for (user, domain), provisioning it on first
// use so admins/users need not pre-add domains. Returns the subdomain, an HTTP status to
// emit on failure (0 on success), and an error code string.
func (a *API) getOrCreateSubdomain(user certo.User, key certo.APIKey, domain string) (string, int, string) {
	if !key.HasDomainAccess(domain) {
		return "", http.StatusForbidden, "domain_not_in_scope"
	}
	if sub, err := a.DB.GetSubdomainByUserDomain(user.ID, domain); err == nil {
		return sub, 0, ""
	}
	// Domain not registered yet: provision it (scope was verified above, so any in-scope
	// domain — exact or wildcard — may be created).
	ud, err := a.DB.AddUserDomain(user.ID, user.Username, domain)
	if err != nil {
		a.Logger.Errorw("acme-dns: auto-provision failed",
			"user", user.Username, "domain", domain, "error", err.Error())
		return "", http.StatusInternalServerError, "db_error"
	}
	a.Logger.Infow("acme-dns: domain auto-provisioned",
		"user", user.Username, "domain", domain, "subdomain", ud.Subdomain)
	return ud.Subdomain, 0, ""
}

// acmednsStorageFetch handles GET /acmedns/:domain (lego HTTP storage Fetch).
// Basic-auth'd; provisions the domain on demand and returns its account.
func (a *API) acmednsStorageFetch(w http.ResponseWriter, r *http.Request, ps httprouter.Params) {
	username, secret, ok := r.BasicAuth()
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	user, key, ok := a.authUserKey(username, secret)
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	domain := normalizeACMEDNSDomain(ps.ByName("domain"))
	if domain == "" {
		jsonResp(w, http.StatusNotFound, map[string]string{"error": "domain_not_found"})
		return
	}
	sub, status, errCode := a.getOrCreateSubdomain(user, key, domain)
	if status != 0 {
		jsonResp(w, status, map[string]string{"error": errCode})
		return
	}
	jsonResp(w, http.StatusOK, a.buildAccount(r, username, secret, sub))
}

// acmednsStorageFetchAll handles GET /acmedns (lego HTTP storage FetchAll).
func (a *API) acmednsStorageFetchAll(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	username, secret, ok := r.BasicAuth()
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	user, key, ok := a.authUserKey(username, secret)
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	domains, err := a.DB.GetUserDomains(user.ID)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	mapping := make(map[string]certo.ACMEDNSAccount, len(domains))
	for _, d := range domains {
		// Only expose domains the authenticating key is scoped for (global keys see all),
		// matching apiGetDomains and the single-domain storage fetch.
		if !key.HasDomainAccess(d.Domain) {
			continue
		}
		mapping[d.Domain] = a.buildAccount(r, username, secret, d.Subdomain)
	}
	jsonResp(w, http.StatusOK, mapping)
}

// acmednsStoragePut handles POST /acmedns/:domain (lego HTTP storage Put). Not hit
// in the normal flow (Fetch already 200s); implemented as an idempotent get-or-create for
// robustness. Returns 200 (lego then prompts the one-time CNAME setup if it was a register).
func (a *API) acmednsStoragePut(w http.ResponseWriter, r *http.Request, ps httprouter.Params) {
	username, secret, ok := r.BasicAuth()
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	user, key, ok := a.authUserKey(username, secret)
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	domain := normalizeACMEDNSDomain(ps.ByName("domain"))
	if domain == "" {
		jsonResp(w, http.StatusNotFound, map[string]string{"error": "domain_not_found"})
		return
	}
	sub, status, errCode := a.getOrCreateSubdomain(user, key, domain)
	if status != 0 {
		jsonResp(w, status, map[string]string{"error": errCode})
		return
	}
	jsonResp(w, http.StatusOK, a.buildAccount(r, username, secret, sub))
}

// clientIP returns the caller's IP, honoring the configured forwarded header.
func (a *API) clientIP(r *http.Request) string {
	if a.Config.API.UseHeader && a.Config.API.HeaderName != "" {
		if v := r.Header.Get(a.Config.API.HeaderName); v != "" {
			if i := strings.IndexByte(v, ','); i >= 0 {
				v = v[:i]
			}
			return strings.TrimSpace(v)
		}
	}
	if host, _, err := net.SplitHostPort(r.RemoteAddr); err == nil {
		return host
	}
	return r.RemoteAddr
}

// ipAllowed reports whether ip falls within any of the allow-list CIDRs. An empty list
// allows everything (acme-dns semantics: no restriction).
func ipAllowed(ip string, cidrs []string) bool {
	if len(cidrs) == 0 {
		return true
	}
	parsed := net.ParseIP(ip)
	if parsed == nil {
		return false
	}
	for _, c := range cidrs {
		if _, network, err := net.ParseCIDR(strings.TrimSpace(c)); err == nil && network.Contains(parsed) {
			return true
		}
	}
	return false
}

// acmednsRegister handles POST /register (native acme-dns). Anonymous: it allocates a fresh
// acme-<nanoid> account bound to a random 10-char subdomain and returns the credentials, so
// stock acme-dns clients (local file storage) work without any pre-existing certo account.
func (a *API) acmednsRegister(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	if !a.Config.API.RegistrationAllowed() {
		jsonResp(w, http.StatusForbidden, map[string]string{"error": "registration_disabled"})
		return
	}
	var req certo.ACMEDNSRegisterRequest
	// Body is optional (acme-dns allows an empty POST); ignore decode errors.
	_ = json.NewDecoder(r.Body).Decode(&req)
	if req.AllowFrom == nil {
		req.AllowFrom = []string{}
	}

	username, apiKey, subdomain, err := a.DB.CreateACMEDNSAccount(req.AllowFrom)
	if err != nil {
		a.Logger.Errorw("acme-dns register: DB error", "error", err.Error())
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	fullDomain := certo.InternalDomain(subdomain, a.Config.General.Domain)
	a.Logger.Infow("acme-dns account registered",
		"username", username, "subdomain", subdomain, "client_ip", a.clientIP(r))
	jsonResp(w, http.StatusCreated, certo.ACMEDNSRegisterResponse{
		Username:   username,
		Password:   apiKey,
		FullDomain: fullDomain,
		SubDomain:  subdomain,
		AllowFrom:  req.AllowFrom,
	})
}

// acmednsUpdate handles POST /update (native acme-dns / lego provider UpdateTXTRecord).
// Auth via X-Api-User/X-Api-Key headers; writes the TXT for the caller's subdomain.
func (a *API) acmednsUpdate(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, key, ok := a.authUserKey(r.Header.Get("X-Api-User"), r.Header.Get("X-Api-Key"))
	if !ok {
		acmednsUnauthorized(w)
		return
	}
	var req certo.ACMEDNSUpdateRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	if len(req.Txt) != certo.ACMEDNSTxtLength {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "bad_txt"})
		return
	}
	subdomain := strings.ToLower(strings.TrimSpace(req.SubDomain))
	owner, domain, allowFrom, err := a.DB.GetUserDomainBySubdomain(subdomain)
	if err != nil || owner != user.ID {
		acmednsForbidden(w)
		return
	}
	if !key.HasDomainAccess(domain) {
		jsonResp(w, http.StatusForbidden, map[string]string{"error": "domain_not_in_scope"})
		return
	}
	// Enforce the acme-dns source-IP allow list (empty = allow from anywhere).
	if !ipAllowed(a.clientIP(r), allowFrom) {
		a.Logger.Warnw("acme-dns update: client IP not in allowfrom",
			"user", user.Username, "subdomain", subdomain, "client_ip", a.clientIP(r))
		jsonResp(w, http.StatusForbidden, map[string]string{"error": "ip_not_allowed"})
		return
	}
	if err := a.DB.UpdateACMEDNSTXT(certo.InternalDomain(subdomain, a.Config.General.Domain), req.Txt); err != nil {
		a.Logger.Errorw("acme-dns update: DB error", "error", err.Error())
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	a.Logger.Infow("acme-dns TXT updated", "user", user.Username, "subdomain", subdomain)
	jsonResp(w, http.StatusOK, certo.ACMEDNSUpdateResponse{Txt: req.Txt})
}

func acmednsUnauthorized(w http.ResponseWriter) {
	w.Header().Set("WWW-Authenticate", `Basic realm="acmedns"`)
	jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "unauthorized"})
}

func acmednsForbidden(w http.ResponseWriter) {
	jsonResp(w, http.StatusForbidden, map[string]string{"error": "forbidden"})
}
