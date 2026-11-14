// web is the shop fixture's frontend stand-in: one page, rendered on the
// server from the api's product list. It exists so that one app depends on
// another.
package main

import (
	"encoding/json"
	"html/template"
	"log/slog"
	"net"
	"net/http"
	"os"
	"time"
)

var page = template.Must(template.New("page").Parse(`<!doctype html>
<title>shop</title>
<h1>shop</h1>
<ul>{{range .}}<li>{{.Name}} — {{.Price}}</li>{{end}}</ul>
`))

type product struct {
	Name  string `json:"name"`
	Price int    `json:"price"`
}

func main() {
	log := slog.New(slog.NewTextHandler(os.Stdout, nil))
	api := os.Getenv("API_URL")
	client := &http.Client{Timeout: 5 * time.Second}

	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
	mux.HandleFunc("GET /{$}", func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		resp, err := client.Get(api + "/products")
		if err != nil {
			log.Error("api unreachable", "err", err)
			http.Error(w, "the api is not answering: "+err.Error(), http.StatusBadGateway)
			return
		}
		defer resp.Body.Close()
		var products []product
		if err := json.NewDecoder(resp.Body).Decode(&products); err != nil {
			log.Error("decode products", "status", resp.StatusCode, "err", err)
			http.Error(w, "the api sent something unexpected", http.StatusBadGateway)
			return
		}
		_ = page.Execute(w, products)
		log.Info("GET /", "products", len(products), "took", time.Since(start).Round(time.Millisecond))
	})

	// doze starts the app with where to listen: a private port it forwards to.
	addr := net.JoinHostPort(os.Getenv("HOST"), os.Getenv("PORT"))
	log.Info("listening", "addr", addr, "api", api)
	if err := http.ListenAndServe(addr, mux); err != nil {
		log.Error("serve", "err", err)
		os.Exit(1)
	}
}
