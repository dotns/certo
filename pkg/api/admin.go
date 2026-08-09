package api

import (
	"crypto/subtle"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
	"golang.org/x/crypto/bcrypt"
)

// AdminAuth middleware checks the X-Admin-Key header against the configured admin_key.
func (a *API) AdminAuth(next httprouter.Handle) httprouter.Handle {
	return func(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
		if a.Config.API.AdminKey == "" {
			jsonResp(w, http.StatusForbidden, map[string]string{"error": "admin_key_not_configured"})
			return
		}
		key := r.Header.Get("X-Admin-Key")
		if subtle.ConstantTimeCompare([]byte(key), []byte(a.Config.API.AdminKey)) != 1 {
			jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "invalid_admin_key"})
			return
		}
		next(w, r, p)
	}
}

// GET /admin/users
func (a *API) adminListUsers(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	users, err := a.DB.ListUsers()
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if users == nil {
		users = []certo.User{}
	}
	jsonResp(w, http.StatusOK, users)
}

// POST /admin/users — create user with {"username":"x","password":"y"}
func (a *API) adminCreateUser(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	var req struct {
		Username string `json:"username"`
		Password string `json:"password"`
	}
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
		jsonResp(w, http.StatusConflict, map[string]string{"error": "username_taken"})
		return
	}
	jsonResp(w, http.StatusCreated, user)
}

// DELETE /admin/users/:id
func (a *API) adminDeleteUser(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
	id, err := strconv.ParseInt(p.ByName("id"), 10, 64)
	if err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_user_id"})
		return
	}
	if err := a.DB.DeleteUser(id); err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// GET /admin/domains
func (a *API) adminListDomains(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	domains, err := a.DB.ListAllDomains()
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if domains == nil {
		domains = []certo.UserDomain{}
	}
	for i := range domains {
		domains[i].CNAMETarget = certo.InternalDomain(domains[i].Subdomain, a.Config.General.Domain)
	}
	jsonResp(w, http.StatusOK, domains)
}

// POST /admin/domains — add domain for a user {"user_id":1,"domain":"example.com"}
func (a *API) adminAddDomain(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	var req struct {
		UserID int64  `json:"user_id"`
		Domain string `json:"domain"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	domain := strings.ToLower(strings.TrimSpace(req.Domain))
	if req.UserID == 0 || domain == "" {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "user_id and domain required"})
		return
	}
	user, err := a.DB.GetUserByID(req.UserID)
	if err != nil {
		jsonResp(w, http.StatusNotFound, map[string]string{"error": "user_not_found"})
		return
	}
	ud, err := a.DB.AddUserDomain(req.UserID, user.Username, domain)
	if err != nil {
		if strings.Contains(err.Error(), "UNIQUE") || strings.Contains(err.Error(), "unique") {
			jsonResp(w, http.StatusConflict, map[string]string{"error": "domain_already_exists"})
			return
		}
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	ud.CNAMETarget = certo.InternalDomain(ud.Subdomain, a.Config.General.Domain)
	jsonResp(w, http.StatusCreated, ud)
}

// DELETE /admin/domains/:domain
func (a *API) adminRemoveDomain(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
	// Find owner first, then delete
	domain := strings.ToLower(strings.TrimSpace(p.ByName("domain")))
	// Try to find all users that have this domain and remove from each
	users, _ := a.DB.ListUsers()
	for _, u := range users {
		_ = a.DB.RemoveUserDomain(u.ID, domain)
	}
	w.WriteHeader(http.StatusNoContent)
}

// GET /admin/records
func (a *API) adminListRecords(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	records, err := a.DB.ListTXTRecords()
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if records == nil {
		records = []certo.TXTRecord{}
	}
	jsonResp(w, http.StatusOK, records)
}
