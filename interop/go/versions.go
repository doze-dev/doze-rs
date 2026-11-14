package main

// The Kafka major the broker stands for (run.sh -kafka-version N, which starts it with --kafka-version N and passes
// -kafka-major N here). It decides two things: how the clients are configured, as a user of that major would configure
// them, and which scenarios run at all, which the broker itself decides by what it advertises over ApiVersions.

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	lib "github.com/IBM/sarama"
	"github.com/twmb/franz-go/pkg/kgo"
	"github.com/twmb/franz-go/pkg/kmsg"
)

// kafkaMajor is the major of -kafka-major (the broker's default, 4, unless told).
var kafkaMajor = 4

// saramaVersion is the cfg.Version a sarama user sets for a broker of the major: sarama does not negotiate every API
// from ApiVersions, it picks request versions from this, so it must not claim a newer broker than the one it talks to.
// The last release of each major (1.1, 2.8, 3.9), which are the brokers the version table was recorded from.
func saramaVersion() lib.KafkaVersion {
	switch kafkaMajor {
	case 1:
		return lib.V1_1_0_0
	case 2:
		return lib.V2_8_0_0
	case 3:
		return lib.V3_9_0_0
	default:
		return lib.V4_0_0_0
	}
}

// saramaConfig is sarama's default configuration, for the major.
func saramaConfig() *lib.Config {
	cfg := lib.NewConfig()
	cfg.Version = saramaVersion()
	return cfg
}

// apis is what the broker advertises: each API key's lowest and highest version.
type apis map[int16][2]int16

// offered is what the broker under test advertises, asked once at the start (nil: not asked, everything assumed).
var offered apis

// askOffered sends ApiVersions with franz-go (logged in with opts, if the broker wants SASL or TLS).
func askOffered(addr string, opts ...kgo.Opt) (apis, error) {
	cl, err := kgo.NewClient(append(opts, kgo.SeedBrokers(addr))...)
	if err != nil {
		return nil, err
	}
	defer cl.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	resp, err := kmsg.NewPtrApiVersionsRequest().RequestWith(ctx, cl)
	if err != nil {
		return nil, err
	}
	if resp.ErrorCode != 0 {
		return nil, fmt.Errorf("ApiVersions: error %d", resp.ErrorCode)
	}
	out := apis{}
	for _, k := range resp.ApiKeys {
		out[k.ApiKey] = [2]int16{k.MinVersion, k.MaxVersion}
	}
	return out, nil
}

// has says whether the broker advertises key (true when it was not asked).
func (a apis) has(key kmsg.Key) bool {
	if a == nil {
		return true
	}
	_, ok := a[int16(key)]
	return ok
}

// missing names the keys of want the broker does not advertise.
func (a apis) missing(want []kmsg.Key) []string {
	var out []string
	for _, k := range want {
		if !a.has(k) {
			out = append(out, k.Name())
		}
	}
	return out
}

// summary is the advertised list in one line, for the log.
func (a apis) summary() string {
	keys := make([]int, 0, len(a))
	for k := range a {
		keys = append(keys, int(k))
	}
	sort.Ints(keys)
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		r := a[int16(k)]
		parts = append(parts, fmt.Sprintf("%d:%d-%d", k, r[0], r[1]))
	}
	return strings.Join(parts, " ")
}

// needer is a scenario that uses APIs not every major has; it is skipped, with the reason, where they are not offered.
type needer interface {
	needs() []kmsg.Key
}

// The APIs of the new consumer group protocol and of transactions, which the scenarios ask for.
var (
	kip848Keys = []kmsg.Key{kmsg.ConsumerGroupHeartbeat}
	txnKeys    = []kmsg.Key{kmsg.InitProducerID, kmsg.AddPartitionsToTxn, kmsg.EndTxn}
	eosKeys    = append(append([]kmsg.Key{}, txnKeys...), kmsg.AddOffsetsToTxn, kmsg.TxnOffsetCommit)
	usersKeys  = []kmsg.Key{kmsg.DescribeUserSCRAMCredentials, kmsg.AlterUserSCRAMCredentials}
)

// clientNeeds is what the members of a group need: the new protocol's heartbeat, if one of them speaks it.
func clientNeeds(members []client) []kmsg.Key {
	for _, m := range members {
		if f, ok := m.(franz); ok && f.kip848 {
			return kip848Keys
		}
		if r, ok := m.(rdkafka); ok && r.kip848 {
			return kip848Keys
		}
	}
	return nil
}

func (s scenario) needs() []kmsg.Key { return clientNeeds(s.members) }
func (c churn) needs() []kmsg.Key    { return clientNeeds(c.clients) }
func (eos) needs() []kmsg.Key        { return eosKeys }
func (fence) needs() []kmsg.Key      { return txnKeys }
func (txnScenario) needs() []kmsg.Key {
	return txnKeys
}

// skipReason is why r cannot run against this broker, or "" if it can.
func skipReason(r runner) string {
	n, ok := r.(needer)
	if !ok {
		return ""
	}
	if gone := offered.missing(n.needs()); len(gone) > 0 {
		return fmt.Sprintf("Kafka %d does not offer %s", kafkaMajor, strings.Join(gone, ", "))
	}
	return ""
}
