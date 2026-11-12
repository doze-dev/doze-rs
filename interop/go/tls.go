package main

// TLS mode (-tls-ca FILE -broker-ssl HOST:PORT): the cells of sasl.go over TLS. Two brokers are running (interop/run.sh -tls):
// the one at -broker has TLS and SASL (SCRAM-SHA-512), a SASL_SSL listener; the one at -broker-ssl has TLS alone, an SSL
// listener. Every library trusts the throwaway CA of the script (and only it) and, for each listener, writes 20 records,
// reads them back, and then fails in the way that listener allows: a wrong password on SASL_SSL, a client that does not
// trust the CA on SSL.

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"os"
	"strings"
	"time"
)

// secure is what the clients of sasl.go connect with: nil for plaintext, else the trust to use. caFile is the same CA as a file,
// for librdkafka.
var secure struct {
	cfg    *tls.Config
	caFile string
}

// runTLS runs every cell and reports like the SASL ones; it returns whether all passed.
func runTLS(saslSSL, ssl, caFile string) bool {
	pem, err := os.ReadFile(caFile)
	if err != nil {
		fmt.Println("FAIL tls: cannot read the CA:", err)
		return false
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(pem) {
		fmt.Println("FAIL tls: no certificate in", caFile)
		return false
	}
	secure.cfg, secure.caFile = &tls.Config{RootCAs: pool, MinVersion: tls.VersionTLS12}, caFile
	ok := true
	clients := []saslClient{saramaSasl{}, franzSasl{}, kafkaGoSasl{}, kcatSasl{}}
	cells := []struct{ shape, addr, mech, refusal string }{
		{"SASL_SSL", saslSSL, "SCRAM-SHA-512", "a wrong password"},
		{"SSL", ssl, "", "a client that does not trust the CA"},
	}
	for _, shape := range cells {
		for _, c := range clients {
			title := c.name() + "/" + shape.shape
			start := time.Now()
			refused, err := cell(shape.addr, c, shape.mech)
			if err != nil {
				fmt.Printf("FAIL tls  %-32s %v\n", title, err)
				ok = false
				continue
			}
			fmt.Printf("ok   tls  %-32s %4.1fs  wrote and read %d records; %s: %s\n", title, time.Since(start).Seconds(), records, shape.refusal, refused)
		}
	}
	return ok
}

// untrustedCA connects as c does but without the CA of the script: it must fail, and as a certificate failure (not, say, a
// broker that is down).
func untrustedCA(ctx context.Context, addr string, c saslClient) (string, error) {
	saved := secure
	defer func() { secure = saved }()
	secure.cfg, secure.caFile = &tls.Config{MinVersion: tls.VersionTLS12}, ""
	try, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	began := time.Now()
	err := c.login(try, addr, "", "")
	if err == nil {
		return "", fmt.Errorf("a client that does not trust the CA was served")
	}
	words := strings.Join(strings.Fields(err.Error()), " ")
	lower := strings.ToLower(words)
	if !strings.Contains(lower, "certificate") && !strings.Contains(lower, "x509") && !strings.Contains(lower, "ssl") && !strings.Contains(lower, "tls") {
		return "", fmt.Errorf("an untrusting client failed, but not as a certificate failure (%v): %s", time.Since(began), words)
	}
	if len(words) > 90 {
		words = words[:90] + "..."
	}
	return words, nil
}
