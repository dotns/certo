package api

import (
	"encoding/json"
	"net/http"
	"strings"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
	"golang.org/x/crypto/bcrypt"
)

type registerRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

type loginResponse struct {
	Token    string `json:"token"`
	Username string `json:"username"`
}

type addDomainRequest struct {
	Domain string `json:"domain"`
}

// POST /api/register
func (a *API) apiRegister(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	if !a.Config.API.RegistrationAllowed() {
		jsonResp(w, http.StatusForbidden, map[string]string{"error": "registration_disabled"})
		return
	}
	var req registerRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	req.Username = strings.TrimSpace(req.Username)
	if req.Username == "" || len(req.Password) < 6 {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "username required, password min 6 chars"})
		return
	}
	hash, err := bcrypt.GenerateFromPassword([]byte(req.Password), 10)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "internal_error"})
		return
	}
	user, err := a.DB.CreateUser(req.Username, string(hash))
	if err != nil {
		a.Logger.Errorw("Register: DB error", "error", err.Error())
		jsonResp(w, http.StatusConflict, map[string]string{"error": "username_taken"})
		return
	}
	token, err := a.generateToken(user.ID, user.Username)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "token_error"})
		return
	}
	jsonResp(w, http.StatusCreated, loginResponse{Token: token, Username: user.Username})
}

// POST /api/login
func (a *API) apiLogin(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	var req registerRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	user, err := a.DB.GetUserByUsername(strings.TrimSpace(req.Username))
	if err != nil {
		bcrypt.CompareHashAndPassword([]byte("$2a$10$placeholder_hash_for_timing"), []byte(req.Password))
		jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "invalid_credentials"})
		return
	}
	if err := bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(req.Password)); err != nil {
		jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "invalid_credentials"})
		return
	}
	token, err := a.generateToken(user.ID, user.Username)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "token_error"})
		return
	}
	jsonResp(w, http.StatusOK, loginResponse{Token: token, Username: user.Username})
}

// GET /api/profile
func (a *API) apiProfile(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	jsonResp(w, http.StatusOK, map[string]interface{}{
		"username": user.Username,
	})
}

// DELETE /api/profile
func (a *API) apiDeleteAccount(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	// Clean up TXT records for all user domains
	domains, _ := a.DB.GetUserDomains(user.ID)
	for _, d := range domains {
		internalDomain := certo.InternalDomain(d.Subdomain, a.Config.General.Domain)
		txts, _ := a.DB.GetTXTForDomain(internalDomain)
		for _, v := range txts {
			_ = a.DB.CleanupTXT(internalDomain, v)
		}
	}
	if err := a.DB.DeleteUser(user.ID); err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// GET /api/domains
func (a *API) apiGetDomains(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	domains, err := a.DB.GetUserDomains(user.ID)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if domains == nil {
		domains = []certo.UserDomain{}
	}
	// Filter by API key scope if present
	if key, ok := getAPIKeyFromContext(r); ok && !key.IsGlobal() {
		filtered := make([]certo.UserDomain, 0)
		for _, d := range domains {
			if key.HasDomainAccess(d.Domain) {
				filtered = append(filtered, d)
			}
		}
		domains = filtered
	}
	for i := range domains {
		domains[i].CNAMETarget = certo.InternalDomain(domains[i].Subdomain, a.Config.General.Domain)
	}
	jsonResp(w, http.StatusOK, domains)
}

// POST /api/domains
func (a *API) apiAddDomain(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	var req addDomainRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	domain := strings.ToLower(strings.TrimSpace(req.Domain))
	if domain == "" {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "domain_required"})
		return
	}
	// A scoped key may only add domains within its scope (global keys and JWT pass).
	// Scope changes go through the key-management endpoints, which require JWT or a global
	// key — never as a side effect of adding a domain.
	if key, ok := getAPIKeyFromContext(r); ok && !key.HasDomainAccess(domain) {
		jsonResp(w, http.StatusForbidden, map[string]string{"error": "domain_not_in_scope"})
		return
	}
	ud, err := a.DB.AddUserDomain(user.ID, user.Username, domain)
	if err != nil {
		if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
			jsonResp(w, http.StatusConflict, map[string]string{"error": "domain_already_exists"})
			return
		}
		a.Logger.Errorw("AddDomain: DB error", "error", err.Error())
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	ud.CNAMETarget = certo.InternalDomain(ud.Subdomain, a.Config.General.Domain)
	jsonResp(w, http.StatusCreated, ud)
}

// DELETE /api/domains/:domain
func (a *API) apiRemoveDomain(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
	user, _ := getUserFromContext(r)
	domain := strings.ToLower(strings.TrimSpace(p.ByName("domain")))
	// Scoped key can only delete domains in its scope
	if key, ok := getAPIKeyFromContext(r); ok && !key.IsGlobal() {
		if !key.HasDomainAccess(domain) {
			jsonResp(w, http.StatusForbidden, map[string]string{"error": "domain_not_in_scope"})
			return
		}
	}
	if err := a.DB.RemoveUserDomain(user.ID, domain); err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// GET /api/records
func (a *API) apiGetRecords(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	domains, err := a.DB.GetUserDomains(user.ID)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	// Restrict a scoped key to records within its scope (global keys and JWT see all),
	// mirroring apiGetDomains and the acme-dns storage endpoints.
	key, hasKey := getAPIKeyFromContext(r)
	internalDomains := make([]string, 0, len(domains))
	for _, d := range domains {
		if hasKey && !key.HasDomainAccess(d.Domain) {
			continue
		}
		internalDomains = append(internalDomains, certo.InternalDomain(d.Subdomain, a.Config.General.Domain))
	}
	records, err := a.DB.ListTXTRecordsByDomains(internalDomains)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if records == nil {
		records = []certo.TXTRecord{}
	}
	jsonResp(w, http.StatusOK, records)
}

// GET /api/info
func (a *API) apiInfo(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	apiDomain := a.Config.API.Domain
	if apiDomain == "" {
		apiDomain = a.Config.General.Domain
	}
	jsonResp(w, http.StatusOK, map[string]interface{}{
		"provider":    "ns-certo",
		"version":     a.Version,
		"base_domain": a.Config.General.Domain,
		"api_domain":  apiDomain,
		"capabilities": []string{
			"multi_key",
			"scoped_key",
			"domain_management",
			"wildcard_scope",
			"account_deletion",
			"acmedns",
		},
	})
}

func jsonResp(w http.ResponseWriter, status int, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(data)
}
