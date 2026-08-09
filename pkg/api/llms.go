package api

import (
	_ "embed"
	"net/http"

	"github.com/julienschmidt/httprouter"
)

//go:embed llms-api.txt
var llmsTxt []byte

func (a *API) serveLLMsTxt(w http.ResponseWriter, r *http.Request, _ httprouter.Params) {
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(llmsTxt)
}
