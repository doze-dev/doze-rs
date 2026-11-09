package main

import (
	"context"
	"fmt"
	"os/exec"
	"sort"
	"strings"
	"time"

	lib "github.com/IBM/sarama"
	"github.com/twmb/franz-go/pkg/kgo"
)

// A transactional producer writes three transactions to a topic of three partitions: one committed, one aborted, one
// committed. A consumer reading committed records must see the first and the third and none of the second; one
// reading everything sees all three. franz-go, sarama and librdkafka read both ways. (kafka-go is left out: it reads the
// list of aborted transactions and never uses it, and hands the transaction markers over as messages.)

const (
	committedFirst  = 10 // c0..c9
	abortedMiddle   = 10 // a0..a9
	committedSecond = 5  // d0..d4
)

// txnProducer is a library's transactional producer.
type txnProducer interface {
	name() string
	// run writes the three transactions with the transactional id.
	run(ctx context.Context, addr, topic, id string) error
}

// txnReader is a library's reader of a whole topic from its start, with the isolation asked for.
type txnReader interface {
	name() string
	read(addr, topic string, committed bool) ([]string, error)
}

type txnScenario struct {
	producer txnProducer
}

func (s txnScenario) title() string { return "txn-" + s.producer.name() }

func (s txnScenario) run(addr string) (string, error) {
	ctx, stop := context.WithTimeout(context.Background(), 90*time.Second)
	defer stop()
	topic := fmt.Sprintf("%s-%d", s.title(), time.Now().UnixNano())
	adm, err := newAdmin(addr)
	if err != nil {
		return "", err
	}
	defer adm.close()
	if err := adm.createTopic(topic, 3); err != nil {
		return "", err
	}
	if err := s.producer.run(ctx, addr, topic, topic+"-id"); err != nil {
		return "", fmt.Errorf("producing: %w", err)
	}
	var seen []string
	for _, r := range []txnReader{franzReader{}, saramaReader{}, kcatReader{}} {
		got, err := r.read(addr, topic, true)
		if err != nil {
			return "", fmt.Errorf("%s reading committed: %w", r.name(), err)
		}
		if err := wantExactly(got, "c", committedFirst, "d", committedSecond); err != nil {
			return "", fmt.Errorf("%s reading committed: %w", r.name(), err)
		}
		all, err := r.read(addr, topic, false)
		if err != nil {
			return "", fmt.Errorf("%s reading everything: %w", r.name(), err)
		}
		if err := wantExactly(all, "c", committedFirst, "a", abortedMiddle, "d", committedSecond); err != nil {
			return "", fmt.Errorf("%s reading everything: %w", r.name(), err)
		}
		seen = append(seen, r.name())
	}
	return fmt.Sprintf("read by %s: committed %d, everything %d", strings.Join(seen, ", "),
		committedFirst+committedSecond, committedFirst+abortedMiddle+committedSecond), nil
}

// wantExactly checks got holds each value once: prefix then 0..n-1, for each pair given.
func wantExactly(got []string, spec ...any) error {
	want := map[string]bool{}
	for i := 0; i+1 < len(spec); i += 2 {
		for n := 0; n < spec[i+1].(int); n++ {
			want[fmt.Sprintf("%s%d", spec[i].(string), n)] = true
		}
	}
	sort.Strings(got)
	seen := map[string]int{}
	for _, v := range got {
		seen[v]++
	}
	for v, n := range seen {
		if !want[v] {
			return fmt.Errorf("saw %q, which it should not have (%d records in all)", v, len(got))
		}
		if n != 1 {
			return fmt.Errorf("saw %q %d times", v, n)
		}
	}
	for v := range want {
		if seen[v] == 0 {
			return fmt.Errorf("never saw %q (%d records in all)", v, len(got))
		}
	}
	return nil
}

func values(prefix string, n int) []string {
	out := make([]string, n)
	for i := range out {
		out[i] = fmt.Sprintf("%s%d", prefix, i)
	}
	return out
}

// --- producers ---

type franzTxn struct{}

func (franzTxn) name() string { return "franz-go" }

func (franzTxn) run(ctx context.Context, addr, topic, id string) error {
	cl, err := kgo.NewClient(kgo.SeedBrokers(addr), kgo.TransactionalID(id), kgo.DefaultProduceTopic(topic), kgo.RecordPartitioner(kgo.RoundRobinPartitioner()))
	if err != nil {
		return err
	}
	defer cl.Close()
	one := func(prefix string, n int, commit kgo.TransactionEndTry) error {
		if err := cl.BeginTransaction(); err != nil {
			return err
		}
		recs := make([]*kgo.Record, 0, n)
		for _, v := range values(prefix, n) {
			recs = append(recs, &kgo.Record{Key: []byte(v), Value: []byte(v)})
		}
		if err := cl.ProduceSync(ctx, recs...).FirstErr(); err != nil {
			return err
		}
		return cl.EndTransaction(ctx, commit)
	}
	if err := one("c", committedFirst, kgo.TryCommit); err != nil {
		return err
	}
	if err := one("a", abortedMiddle, kgo.TryAbort); err != nil {
		return err
	}
	return one("d", committedSecond, kgo.TryCommit)
}

type saramaTxn struct{}

func (saramaTxn) name() string { return "sarama" }

func (saramaTxn) run(_ context.Context, addr, topic, id string) error {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	cfg.Producer.Idempotent = true
	cfg.Producer.Transaction.ID = id
	cfg.Producer.RequiredAcks = lib.WaitForAll
	cfg.Net.MaxOpenRequests = 1
	cfg.Producer.Return.Successes = true
	cfg.Producer.Partitioner = lib.NewRoundRobinPartitioner
	p, err := lib.NewAsyncProducer([]string{addr}, cfg)
	if err != nil {
		return err
	}
	defer p.Close()
	one := func(prefix string, n int, commit bool) error {
		if err := p.BeginTxn(); err != nil {
			return err
		}
		for _, v := range values(prefix, n) {
			p.Input() <- &lib.ProducerMessage{Topic: topic, Key: lib.StringEncoder(v), Value: lib.StringEncoder(v)}
		}
		for range n {
			select {
			case <-p.Successes():
			case err := <-p.Errors():
				return err
			case <-time.After(20 * time.Second):
				return fmt.Errorf("no answer for a record")
			}
		}
		if commit {
			return p.CommitTxn()
		}
		return p.AbortTxn()
	}
	if err := one("c", committedFirst, true); err != nil {
		return err
	}
	if err := one("a", abortedMiddle, false); err != nil {
		return err
	}
	return one("d", committedSecond, true)
}

// --- readers ---

const quiet = 2 * time.Second

type franzReader struct{}

func (franzReader) name() string { return "franz-go" }

func (franzReader) read(addr, topic string, committed bool) ([]string, error) {
	level := kgo.ReadUncommitted()
	if committed {
		level = kgo.ReadCommitted()
	}
	cl, err := kgo.NewClient(kgo.SeedBrokers(addr), kgo.ConsumeTopics(topic), kgo.ConsumeResetOffset(kgo.NewOffset().AtStart()), kgo.FetchIsolationLevel(level))
	if err != nil {
		return nil, err
	}
	defer cl.Close()
	var out []string
	for {
		ctx, cancel := context.WithTimeout(context.Background(), quiet)
		fetches := cl.PollFetches(ctx)
		cancel()
		n := 0
		fetches.EachRecord(func(r *kgo.Record) { out = append(out, string(r.Value)); n++ })
		if n == 0 {
			return out, nil
		}
	}
}

type saramaReader struct{}

func (saramaReader) name() string { return "sarama" }

func (saramaReader) read(addr, topic string, committed bool) ([]string, error) {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	cfg.Consumer.IsolationLevel = lib.ReadUncommitted
	if committed {
		cfg.Consumer.IsolationLevel = lib.ReadCommitted
	}
	c, err := lib.NewConsumer([]string{addr}, cfg)
	if err != nil {
		return nil, err
	}
	defer c.Close()
	var out []string
	for p := int32(0); p < 3; p++ {
		pc, err := c.ConsumePartition(topic, p, lib.OffsetOldest)
		if err != nil {
			return nil, err
		}
	drain:
		for {
			select {
			case m := <-pc.Messages():
				out = append(out, string(m.Value))
			case <-time.After(quiet):
				break drain
			}
		}
		_ = pc.Close()
	}
	return out, nil
}

type kcatReader struct{}

func (kcatReader) name() string { return "librdkafka" }

func (kcatReader) read(addr, topic string, committed bool) ([]string, error) {
	level := "read_uncommitted"
	if committed {
		level = "read_committed"
	}
	out, err := exec.Command("kcat", "-C", "-b", addr, "-t", topic, "-o", "beginning", "-e", "-q",
		"-X", "isolation.level="+level, "-f", "%s\n").Output()
	if err != nil {
		return nil, fmt.Errorf("kcat -C: %w", err)
	}
	var got []string
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		if line != "" {
			got = append(got, line)
		}
	}
	return got, nil
}
