package api

import (
	"encoding/json"
	"fmt"
	"net/http"

	"github.com/dotns/certo/pkg/certo"
	"github.com/julienschmidt/httprouter"
)

// resolveSubdomain resolves the nanoid subdomain from the FQDN.
// lego may send either the original FQDN (_acme-challenge.pvv.cc.)
// or the CNAME-resolved FQDN (r0hc4bc6.s.example.com.).
func (a *API) resolveSubdomain(userID int64, fqdn string) (string, error) {
	// Case 1: FQDN is under our base domain (CNAME-resolved), e.g. r0hc4bc6.s.example.com
	if sub, ok := certo.ExtractSubdomainFromFQDN(fqdn, a.Config.General.Domain); ok {
		// Verify this subdomain belongs to the authenticated user
		ownerID, err := a.DB.GetSubdomainOwner(sub)
		if err == nil && ownerID == userID {
			return sub, nil
		}
	}

	// Case 2: Original FQDN, e.g. _acme-challenge.pvv.cc
	domain := certo.ExtractDomainFromFQDN(fqdn)
	return a.DB.GetSubdomainByUserDomain(userID, domain)
}

// autoCreateSubdomain provisions a domain for the user on demand. The caller has already
// verified API-key scope for the domain (so scope governs what a key may create), so this
// only guards the case where the FQDN is a CNAME-resolved name under our base domain
// (which has no real domain to create). Returns an error to leave the 403 path intact.
func (a *API) autoCreateSubdomain(user certo.User, fqdn, domain string) (string, error) {
	if _, underBase := certo.ExtractSubdomainFromFQDN(fqdn, a.Config.General.Domain); underBase {
		return "", fmt.Errorf("cannot auto-create a name under the base domain")
	}
	ud, err := a.DB.AddUserDomain(user.ID, user.Username, domain)
	if err != nil {
		// A concurrent create may have won the race; fall back to a fresh lookup.
		if sub, gerr := a.DB.GetSubdomainByUserDomain(user.ID, domain); gerr == nil {
			return sub, nil
		}
		return "", err
	}
	a.Logger.Infow("Present: domain auto-created", "user", user.Username, "domain", domain)
	return ud.Subdomain, nil
}

// webPresentPost handles POST /present (lego httpreq DNS provider).
func (a *API) webPresentPost(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	var payload certo.HTTPReqPayload
	if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
		a.Logger.Errorw("Present: JSON decode error", "error", err.Error())
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write(jsonError("invalid_json"))
		return
	}
	if payload.FQDN == "" || payload.Value == "" {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write(jsonError("missing_fqdn_or_value"))
		return
	}
	// A TXT value longer than one DNS character-string cannot be served, so reject it here
	// rather than poisoning DNS resolution for the subdomain.
	if len(payload.Value) > certo.MaxTXTValueLength {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write(jsonError("value_too_long"))
		return
	}

	user, ok := getUserFromContext(r)
	if !ok {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write(jsonError("unauthorized"))
		return
	}

	// Check API key scope
	domain := certo.ExtractDomainFromFQDN(payload.FQDN)
	key, hasKey := getAPIKeyFromContext(r)
	if hasKey && !key.HasDomainAccess(domain) {
		a.Logger.Errorw("Present: key scope denied",
			"user", user.Username, "fqdn", payload.FQDN)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write(jsonError("domain_not_in_scope"))
		return
	}

	subdomain, err := a.resolveSubdomain(user.ID, payload.FQDN)
	if err != nil {
		// Domain not registered yet: provision it (scope was already verified above).
		subdomain, err = a.autoCreateSubdomain(user, payload.FQDN, domain)
	}
	if err != nil {
		a.Logger.Errorw("Present: domain not authorized",
			"user", user.Username, "fqdn", payload.FQDN)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write(jsonError("domain_not_authorized"))
		return
	}

	internalDomain := certo.InternalDomain(subdomain, a.Config.General.Domain)

	if err := a.DB.PresentTXT(internalDomain, payload.Value); err != nil {
		a.Logger.Errorw("Present: DB error", "error", err.Error())
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write(jsonError("db_error"))
		return
	}

	a.Logger.Infow("TXT record presented",
		"fqdn", payload.FQDN, "internal_domain", internalDomain)

	resp := certo.HTTPReqResponse{
		InternalDomain: internalDomain,
		CNAMETarget:    internalDomain,
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_ = json.NewEncoder(w).Encode(resp)
}
