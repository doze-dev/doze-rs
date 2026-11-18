// Package config is the broker configuration a scenario can ask for, and nothing
// else. Every field here is one the process target (process_target.go) passes to
// the binary as a flag or a test-knob environment variable, so a scenario cannot ask
// for a broker the replay has no way to start.
//
// The fields and their defaults are those of the same names in the Go edition's
// config.Default (github.com/doze-dev/doze-kafka internal/config), which the
// recordings' scenarios were written against.
package config

import "time"

// SASL mechanisms a scenario can ask for.
const (
	SASLPlain       = "PLAIN"
	SASLScramSHA256 = "SCRAM-SHA-256"
	SASLScramSHA512 = "SCRAM-SHA-512"
)

// ACL defaults (--acl-default).
const (
	ACLAllow = "allow"
	ACLDeny  = "deny"
)

// Config is what a scenario's Doze or Replay function changes.
type Config struct {
	GroupInitialRebalanceDelay time.Duration     // --group-initial-rebalance-delay
	ConsumerSessionTimeout     time.Duration     // --consumer-session-timeout
	OffsetsRetention           time.Duration     // --offsets-retention
	ACLDefault                 string            // --acl-default
	SASLMechanisms             []string          // --sasl-mechanisms, comma-separated; none when empty
	Credentials                map[string]string // --sasl-user user:password, one per user
	ACLSuperUsers              []string          // --acl-super-user, one per principal
	Console                    bool              // --console --console-addr 127.0.0.1:0 when true

	// Test knobs: environment variables, no flag.
	OffsetsRetentionCheckInterval time.Duration // DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL
	ConsumerAssignmentInterval    time.Duration // DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL
}

// Default is the configuration a scenario starts from.
func Default() Config {
	return Config{
		GroupInitialRebalanceDelay: 200 * time.Millisecond,
		ConsumerSessionTimeout:     45 * time.Second,
		OffsetsRetention:           7 * 24 * time.Hour,
		ACLDefault:                 ACLAllow,
		Console:                    true,
		ConsumerAssignmentInterval: time.Second,
	}
}
