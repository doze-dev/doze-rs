package main

// Users step (part of -sasl): a user is made over the API, as `kafka-configs.sh --alter --add-config SCRAM-SHA-256=...`
// does, by franz-go's kadm (which salts the password and runs PBKDF2 itself, in Go, so the broker is checked against an
// independent implementation of the client side). Then all four libraries log in as that user with SCRAM-SHA-256 and
// SCRAM-SHA-512 and run the cells of sasl.go, PLAIN is refused for it (a stored SCRAM verifier cannot give back a
// password), the user configured at start cannot be taken over, and the credentials are deleted one mechanism at a
// time, each deletion refusing the next login with that mechanism in every library.
//
// The broker of run.sh -sasl has one user, alice, and no ACLs, so alice (the admin here) may manage users: the open-broker
// posture of the catalogue (SEC-D51).

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/twmb/franz-go/pkg/kadm"
	"github.com/twmb/franz-go/pkg/kgo"
)

const (
	apiUser = "dave"
	apiPass = "dave-secret-pw-from-the-api"
)

// runUsers runs the steps and reports like the other cells; it returns whether all passed.
func runUsers(addr string) bool {
	ok := true
	step := func(title string, f func() (string, error)) {
		start := time.Now()
		detail, err := f()
		if err != nil {
			fmt.Printf("FAIL sasl %-32s %v\n", title, err)
			ok = false
			return
		}
		fmt.Printf("ok   sasl %-32s %4.1fs  %s\n", title, time.Since(start).Seconds(), detail)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Minute)
	defer cancel()
	// The admin is alice, a user configured at start, over PLAIN.
	cl, err := kgo.NewClient(franzSasl{}.opts(addr, "PLAIN", saslPassword())...)
	if err != nil {
		fmt.Printf("FAIL sasl %-32s %v\n", "users/admin", err)
		return false
	}
	defer cl.Close()
	admin := kadm.NewClient(cl)

	alter := func(del []kadm.DeleteSCRAM, up []kadm.UpsertSCRAM) (kadm.AlteredUserSCRAMs, error) {
		return admin.AlterUserSCRAMs(ctx, del, up)
	}
	step("users/create", func() (string, error) {
		// One mechanism a request: a user named twice in one request is refused, as Kafka refuses it.
		for _, m := range []struct {
			mech       kadm.ScramMechanism
			iterations int32
		}{{kadm.ScramSha256, 4096}, {kadm.ScramSha512, 8192}} {
			r, err := alter(nil, []kadm.UpsertSCRAM{{User: apiUser, Mechanism: m.mech, Iterations: m.iterations, Password: apiPass}})
			if err != nil {
				return "", err
			}
			if got, found := r[apiUser]; !found || got.Err != nil {
				return "", fmt.Errorf("%s: %v (%s)", m.mech, got.Err, got.ErrMessage)
			}
		}
		return "SCRAM-SHA-256 (4096) and SCRAM-SHA-512 (8192) made for " + apiUser, nil
	})
	step("users/describe", func() (string, error) {
		all, err := admin.DescribeUserSCRAMs(ctx)
		if err != nil {
			return "", err
		}
		d, a := all[apiUser], all[saslUser]
		if len(d.CredInfos) != 2 || len(a.CredInfos) == 0 {
			return "", fmt.Errorf("listed %v and %v", d.CredInfos, a.CredInfos)
		}
		want := map[kadm.ScramMechanism]int32{kadm.ScramSha256: 4096, kadm.ScramSha512: 8192}
		for _, c := range d.CredInfos {
			if want[c.Mechanism] != c.Iterations {
				return "", fmt.Errorf("%s: %d iterations listed", c.Mechanism, c.Iterations)
			}
		}
		none, err := admin.DescribeUserSCRAMs(ctx, "nobody-at-all")
		if err != nil {
			return "", err
		}
		if none["nobody-at-all"].Err == nil {
			return "", fmt.Errorf("a user with no credential was described without an error")
		}
		return fmt.Sprintf("%v for %s; %v for %s (configured at start); an unknown user: %v", d.CredInfos, apiUser, a.CredInfos, saslUser, none["nobody-at-all"].Err), nil
	})
	step("users/configured-at-start", func() (string, error) {
		r, err := alter(nil, []kadm.UpsertSCRAM{{User: saslUser, Mechanism: kadm.ScramSha256, Password: "taken-over"}})
		if err != nil {
			return "", err
		}
		got := r[saslUser]
		if got.Err == nil || !strings.Contains(got.ErrMessage, "configured at start") {
			return "", fmt.Errorf("the user configured at start was changed or refused oddly: %v %q", got.Err, got.ErrMessage)
		}
		return fmt.Sprintf("%s cannot be changed: %v", saslUser, got.Err), nil
	})

	clients := []saslClient{saramaSasl{}, franzSasl{}, kafkaGoSasl{}, kcatSasl{}}
	// The cells again, as the new user.
	savedUser, savedPass := saslUser, saslPassOver
	saslUser, saslPassOver = apiUser, apiPass
	for _, mech := range []string{"SCRAM-SHA-256", "SCRAM-SHA-512"} {
		for _, c := range clients {
			start := time.Now()
			refused, err := cell(addr, c, mech)
			title := "users/" + c.name() + "/" + mech
			if err != nil {
				fmt.Printf("FAIL sasl %-32s %v\n", title, err)
				ok = false
				continue
			}
			fmt.Printf("ok   sasl %-32s %4.1fs  wrote and read %d records as %s; a wrong password: %s\n", title, time.Since(start).Seconds(), records, apiUser, refused)
		}
	}
	step("users/plain-refused", func() (string, error) {
		for _, c := range clients {
			if err := mustRefuse(ctx, addr, c, "PLAIN", apiPass); err != nil {
				return "", fmt.Errorf("%s: %w", c.name(), err)
			}
		}
		return "PLAIN with the right password is refused in every library", nil
	})
	step("users/delete-512", func() (string, error) {
		r, err := alter([]kadm.DeleteSCRAM{{User: apiUser, Mechanism: kadm.ScramSha512}}, nil)
		if err != nil || r[apiUser].Err != nil {
			return "", fmt.Errorf("%v %v", err, r[apiUser].Err)
		}
		for _, c := range clients {
			if err := mustRefuse(ctx, addr, c, "SCRAM-SHA-512", apiPass); err != nil {
				return "", fmt.Errorf("%s after the deletion: %w", c.name(), err)
			}
			if err := c.login(ctx, addr, "SCRAM-SHA-256", apiPass); err != nil {
				return "", fmt.Errorf("%s with the mechanism that was not deleted: %w", c.name(), err)
			}
		}
		return "SCRAM-SHA-512 refused, SCRAM-SHA-256 still works, in every library", nil
	})
	step("users/delete-256", func() (string, error) {
		r, err := alter([]kadm.DeleteSCRAM{{User: apiUser, Mechanism: kadm.ScramSha256}}, nil)
		if err != nil || r[apiUser].Err != nil {
			return "", fmt.Errorf("%v %v", err, r[apiUser].Err)
		}
		for _, c := range clients {
			if err := mustRefuse(ctx, addr, c, "SCRAM-SHA-256", apiPass); err != nil {
				return "", fmt.Errorf("%s after the deletion: %w", c.name(), err)
			}
		}
		again, err := alter([]kadm.DeleteSCRAM{{User: apiUser, Mechanism: kadm.ScramSha256}}, nil)
		if err != nil || again[apiUser].Err == nil {
			return "", fmt.Errorf("a second deletion was not refused: %v %v", err, again[apiUser].Err)
		}
		gone, err := admin.DescribeUserSCRAMs(ctx, apiUser)
		if err != nil || gone[apiUser].Err == nil {
			return "", fmt.Errorf("the deleted user is still described: %v %v", err, gone[apiUser])
		}
		return "refused in every library; a second deletion: " + again[apiUser].Err.Error(), nil
	})
	saslUser, saslPassOver = savedUser, savedPass
	return ok
}

// mustRefuse tries a login that the broker must turn down as an authentication failure, quickly.
func mustRefuse(ctx context.Context, addr string, c saslClient, mech, pass string) error {
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	err := c.login(ctx, addr, mech, pass)
	if err == nil {
		return fmt.Errorf("the login was accepted")
	}
	if ctx.Err() != nil {
		return fmt.Errorf("the login was not refused within 20 s (%v)", err)
	}
	if !strings.Contains(strings.ToLower(err.Error()), "authentication") {
		return fmt.Errorf("the login failed, but not as an authentication failure: %v", err)
	}
	return nil
}
