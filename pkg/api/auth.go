package api

import (
	"context"
	"net/http"
	"strings"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
)

type contextKey int

const (
	userContextKey   contextKey = 0
	apiKeyContextKey contextKey = 1
)

// JWTOrGlobalKeyAuth allows JWT or global API keys only.
// Used for sensitive endpoints: profile, keys management, account deletion.
func (a *API) JWTOrGlobalKeyAuth(next httprouter.Handle) httprouter.Handle {
	return func(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
		user, apiKey, ok := a.authenticateBearer(r)
		if !ok {
			jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "unauthorized"})
			return
		}
		if apiKey != nil && !apiKey.IsGlobal() {
			jsonResp(w, http.StatusForbidden, map[string]string{"error": "global_key_required"})
			return
		}
		ctx := context.WithValue(r.Context(), userContextKey, user)
		if apiKey != nil {
			ctx = context.WithValue(ctx, apiKeyContextKey, *apiKey)
		}
		next(w, r.WithContext(ctx), p)
	}
}

// JWTOrKeyAuth allows JWT or any API key (global or scoped).
// Used for domain operations and records — scope checks happen in handlers.
func (a *API) JWTOrKeyAuth(next httprouter.Handle) httprouter.Handle {
	return func(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
		user, apiKey, ok := a.authenticateBearer(r)
		if !ok {
			jsonResp(w, http.StatusUnauthorized, map[string]string{"error": "unauthorized"})
			return
		}
		ctx := context.WithValue(r.Context(), userContextKey, user)
		if apiKey != nil {
			ctx = context.WithValue(ctx, apiKeyContextKey, *apiKey)
		}
		next(w, r.WithContext(ctx), p)
	}
}

// authenticateBearer extracts and validates a Bearer token (JWT or API key).
func (a *API) authenticateBearer(r *http.Request) (certo.User, *certo.APIKey, bool) {
	authHeader := r.Header.Get("Authorization")
	if !strings.HasPrefix(authHeader, "Bearer ") {
		return certo.User{}, nil, false
	}
	tokenStr := strings.TrimPrefix(authHeader, "Bearer ")

	// Try JWT
	if claims, err := a.parseToken(tokenStr); err == nil {
		if user, err := a.DB.GetUserByID(claims.UserID); err == nil {
			return user, nil, true
		}
	}

	// Try API key
	if apiKey, err := a.DB.GetAPIKeyByValue(tokenStr); err == nil {
		if user, err := a.DB.GetUserByID(apiKey.UserID); err == nil {
			return user, &apiKey, true
		}
	}

	return certo.User{}, nil, false
}

// BasicAuthHTTPreq middleware for /present and /cleanup endpoints.
func (a *API) BasicAuthHTTPreq(next httprouter.Handle) httprouter.Handle {
	return func(w http.ResponseWriter, r *http.Request, p httprouter.Params) {
		username, secret, ok := r.BasicAuth()
		if !ok {
			w.Header().Set("WWW-Authenticate", `Basic realm="httpreq"`)
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write(jsonError("unauthorized"))
			return
		}
		user, err := a.DB.GetUserByUsername(username)
		if err != nil {
			w.Header().Set("WWW-Authenticate", `Basic realm="httpreq"`)
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusUnauthorized)
			_, _ = w.Write(jsonError("unauthorized"))
			return
		}

		if apiKey, err := a.DB.GetAPIKeyByValue(secret); err == nil && apiKey.UserID == user.ID {
			ctx := context.WithValue(r.Context(), userContextKey, user)
			ctx = context.WithValue(ctx, apiKeyContextKey, apiKey)
			next(w, r.WithContext(ctx), p)
			return
		}

		w.Header().Set("WWW-Authenticate", `Basic realm="httpreq"`)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write(jsonError("unauthorized"))
	}
}

func getUserFromContext(r *http.Request) (certo.User, bool) {
	u, ok := r.Context().Value(userContextKey).(certo.User)
	return u, ok
}

func getAPIKeyFromContext(r *http.Request) (certo.APIKey, bool) {
	k, ok := r.Context().Value(apiKeyContextKey).(certo.APIKey)
	return k, ok
}
