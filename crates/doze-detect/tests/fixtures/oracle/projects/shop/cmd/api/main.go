// api is the shop fixture's HTTP service. It does the least that makes doze's
// behaviour visible: it reads and writes postgres, counts in valkey, and
// publishes to kafka, logging one line per request.
//
// SHOP_FAIL=1 makes it exit 1 at startup, for exercising crash handling.
package main

import (
	"bufio"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	_ "github.com/jackc/pgx/v5/stdlib"
	"github.com/twmb/franz-go/pkg/kgo"
)

const schema = `
CREATE TABLE IF NOT EXISTS products (
	id    text PRIMARY KEY,
	name  text NOT NULL,
	price integer NOT NULL
);
CREATE TABLE IF NOT EXISTS orders (
	id         bigserial PRIMARY KEY,
	product_id text NOT NULL REFERENCES products(id),
	created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO products (id, name, price) VALUES
	('mug', 'Sleepy mug', 1200),
	('cap', 'Night cap', 1850),
	('sock', 'Bed socks', 990)
ON CONFLICT (id) DO NOTHING;`

type product struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Price int    `json:"price"`
}

type order struct {
	ID        int64  `json:"id"`
	ProductID string `json:"product_id"`
}

func main() {
	log := slog.New(slog.NewTextHandler(os.Stdout, nil))
	if os.Getenv("SHOP_FAIL") != "" {
		log.Error("SHOP_FAIL is set, exiting")
		os.Exit(1)
	}

	db, err := sql.Open("pgx", os.Getenv("DATABASE_URL"))
	if err != nil {
		log.Error("open postgres", "err", err)
		os.Exit(1)
	}
	if _, err := db.Exec(schema); err != nil {
		log.Error("apply schema", "err", err)
		os.Exit(1)
	}
	log.Info("postgres connected")

	// The kafka client connects lazily, so creating it does not wake the
	// broker; the first order does.
	events, err := kgo.NewClient(
		kgo.SeedBrokers(strings.Split(os.Getenv("KAFKA_BROKERS"), ",")...),
		kgo.AllowAutoTopicCreation(),
	)
	if err != nil {
		log.Error("kafka client", "err", err)
		os.Exit(1)
	}
	defer events.Close()

	s := &server{log: log, db: db, cache: os.Getenv("CACHE_ADDR"), events: events}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
	mux.HandleFunc("GET /products", s.products)
	mux.HandleFunc("GET /orders", s.orders)
	mux.HandleFunc("POST /orders", s.createOrder)

	// doze starts the app with where to listen: a private port it forwards to.
	addr := net.JoinHostPort(os.Getenv("HOST"), os.Getenv("PORT"))
	log.Info("listening", "addr", addr)
	if err := http.ListenAndServe(addr, s.logged(mux)); err != nil {
		log.Error("serve", "err", err)
		os.Exit(1)
	}
}

type server struct {
	log    *slog.Logger
	db     *sql.DB
	cache  string
	events *kgo.Client
}

func (s *server) logged(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/healthz" {
			next.ServeHTTP(w, r)
			return
		}
		rec := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
		start := time.Now()
		next.ServeHTTP(rec, r)
		s.log.Info(r.Method+" "+r.URL.Path, "status", rec.status, "took", time.Since(start).Round(time.Millisecond))
	})
}

func (s *server) products(w http.ResponseWriter, r *http.Request) {
	rows, err := s.db.QueryContext(r.Context(), `SELECT id, name, price FROM products ORDER BY id`)
	if err != nil {
		s.fail(w, "query products", err)
		return
	}
	defer rows.Close()
	out := []product{}
	for rows.Next() {
		var p product
		if err := rows.Scan(&p.ID, &p.Name, &p.Price); err != nil {
			s.fail(w, "scan product", err)
			return
		}
		out = append(out, p)
	}
	views, err := s.incr(r.Context(), "views:products")
	if err != nil {
		// The cache is an optimisation: say so and carry on.
		s.log.Warn("cache unavailable", "err", err)
	}
	w.Header().Set("X-Views", fmt.Sprint(views))
	writeJSON(w, http.StatusOK, out)
}

func (s *server) orders(w http.ResponseWriter, r *http.Request) {
	rows, err := s.db.QueryContext(r.Context(), `SELECT id, product_id FROM orders ORDER BY id DESC LIMIT 20`)
	if err != nil {
		s.fail(w, "query orders", err)
		return
	}
	defer rows.Close()
	out := []order{}
	for rows.Next() {
		var o order
		if err := rows.Scan(&o.ID, &o.ProductID); err != nil {
			s.fail(w, "scan order", err)
			return
		}
		out = append(out, o)
	}
	writeJSON(w, http.StatusOK, out)
}

func (s *server) createOrder(w http.ResponseWriter, r *http.Request) {
	var in struct {
		ProductID string `json:"product_id"`
	}
	if err := json.NewDecoder(r.Body).Decode(&in); err != nil || in.ProductID == "" {
		http.Error(w, `expected {"product_id": "..."}`, http.StatusBadRequest)
		return
	}
	o := order{ProductID: in.ProductID}
	err := s.db.QueryRowContext(r.Context(),
		`INSERT INTO orders (product_id) VALUES ($1) RETURNING id`, in.ProductID).Scan(&o.ID)
	if err != nil {
		s.fail(w, "insert order", err)
		return
	}
	body, _ := json.Marshal(o)
	rec := &kgo.Record{Topic: "orders", Key: []byte(fmt.Sprint(o.ID)), Value: body}
	if err := s.events.ProduceSync(r.Context(), rec).FirstErr(); err != nil {
		s.fail(w, "publish order", err)
		return
	}
	s.log.Info("published", "topic", "orders", "key", o.ID)
	writeJSON(w, http.StatusCreated, o)
}

// incr bumps a counter in valkey over a plain connection. One command is not
// worth a client library.
func (s *server) incr(ctx context.Context, key string) (int64, error) {
	d := net.Dialer{Timeout: 2 * time.Second}
	conn, err := d.DialContext(ctx, "tcp", s.cache)
	if err != nil {
		return 0, err
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(2 * time.Second))
	if _, err := fmt.Fprintf(conn, "INCR %s\r\n", key); err != nil {
		return 0, err
	}
	line, err := bufio.NewReader(conn).ReadString('\n')
	if err != nil {
		return 0, err
	}
	var n int64
	if _, err := fmt.Sscanf(line, ":%d", &n); err != nil {
		return 0, errors.New("unexpected reply: " + strings.TrimSpace(line))
	}
	return n, nil
}

func (s *server) fail(w http.ResponseWriter, what string, err error) {
	s.log.Error(what, "err", err)
	http.Error(w, what+": "+err.Error(), http.StatusInternalServerError)
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (r *statusRecorder) WriteHeader(status int) {
	r.status = status
	r.ResponseWriter.WriteHeader(status)
}
