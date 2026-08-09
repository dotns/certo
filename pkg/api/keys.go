package api

import (
	"encoding/json"
	"net/http"
	"strconv"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
)

type createKeyRequest struct {
	Name  string   `json:"name"`
	Scope []string `json:"scope"`
}

// GET /api/keys
func (a *API) apiListKeys(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	keys, err := a.DB.ListAPIKeys(user.ID)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	if keys == nil {
		keys = []certo.APIKey{}
	}
	jsonResp(w, http.StatusOK, keys)
}

// POST /api/keys
func (a *API) apiCreateKey(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	user, _ := getUserFromContext(r)
	var req createKeyRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_json"})
		return
	}
	if req.Name == "" {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "name_required"})
		return
	}
	if len(req.Scope) == 0 {
		req.Scope = []string{"*"}
	}
	key, err := a.DB.CreateAPIKey(user.ID, req.Name, req.Scope)
	if err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	jsonResp(w, http.StatusCreated, key)
}

// DELETE /api/keys/:id
func (a *API) apiDeleteKey(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
	user, _ := getUserFromContext(r)
	id, err := strconv.ParseInt(p.ByName("id"), 10, 64)
	if err != nil {
		jsonResp(w, http.StatusBadRequest, map[string]string{"error": "invalid_id"})
		return
	}
	if err := a.DB.DeleteAPIKey(user.ID, id); err != nil {
		jsonResp(w, http.StatusInternalServerError, map[string]string{"error": "db_error"})
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
