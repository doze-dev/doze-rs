package main

import (
	"context"
	"fmt"
	"time"

	"github.com/twmb/franz-go/pkg/kgo"
)

// eos is exactly-once consume-transform-produce with franz-go's GroupTransactSession: records are read from an input
// topic, "<n>" is written to an output topic as "o<n>", and the consumer's offsets commit in the same transaction. One
// transaction in the middle is aborted on purpose, so that its records are read again; at the end the output holds each
// record exactly once for a read-committed reader, and every input offset is committed.
type eos struct{}

func (eos) title() string { return "eos-franz-go" }

func (eos) run(addr string) (string, error) {
	const total = 200
	ctx, stop := context.WithTimeout(context.Background(), 120*time.Second)
	defer stop()
	stamp := time.Now().UnixNano()
	in, out, group := fmt.Sprintf("eos-in-%d", stamp), fmt.Sprintf("eos-out-%d", stamp), fmt.Sprintf("eos-g-%d", stamp)
	adm, err := newAdmin(addr)
	if err != nil {
		return "", err
	}
	defer adm.close()
	for _, topic := range []string{in, out} {
		if err := adm.createTopic(topic, 3); err != nil {
			return "", err
		}
	}
	if err := (franz{addr: addr}).produce(ctx, in, 0, total); err != nil {
		return "", fmt.Errorf("producing the input: %w", err)
	}
	sess, err := kgo.NewGroupTransactSession(
		kgo.SeedBrokers(addr),
		kgo.TransactionalID(group+"-tx"),
		kgo.ConsumerGroup(group),
		kgo.ConsumeTopics(in),
		kgo.ConsumeResetOffset(kgo.NewOffset().AtStart()),
		kgo.FetchIsolationLevel(kgo.ReadCommitted()),
		kgo.DefaultProduceTopic(out),
		kgo.Balancers(kgo.RangeBalancer()),
	)
	if err != nil {
		return "", err
	}
	defer sess.Close()
	done, rounds, aborted := 0, 0, 0
	for done < total {
		if ctx.Err() != nil {
			return "", fmt.Errorf("stopped at %d of %d", done, total)
		}
		fetches := sess.PollRecords(ctx, 25)
		if err := fetches.Err0(); err != nil && ctx.Err() == nil {
			return "", fmt.Errorf("fetching: %w", err)
		}
		if fetches.NumRecords() == 0 {
			continue
		}
		if err := sess.Begin(); err != nil {
			return "", fmt.Errorf("beginning a transaction: %w", err)
		}
		var recs []*kgo.Record
		n := 0
		fetches.EachRecord(func(r *kgo.Record) {
			n++
			recs = append(recs, &kgo.Record{Key: r.Value, Value: append([]byte("o"), r.Value...)})
		})
		if err := sess.ProduceSync(ctx, recs...).FirstErr(); err != nil {
			return "", fmt.Errorf("producing: %w", err)
		}
		rounds++
		// The third transaction is abandoned: its output is aborted and its input read again.
		commit := kgo.TryCommit
		if rounds == 3 {
			commit = kgo.TryAbort
		}
		committed, err := sess.End(ctx, commit)
		if err != nil {
			return "", fmt.Errorf("ending a transaction: %w", err)
		}
		if committed {
			done += n
		} else {
			aborted++
		}
	}
	var wantAll []string
	for i := 0; i < total; i++ {
		wantAll = append(wantAll, fmt.Sprintf("o%d", i))
	}
	got, err := franzReader{}.read(addr, out, true)
	if err != nil {
		return "", err
	}
	if len(got) != total {
		return "", fmt.Errorf("read %d records of output, want %d", len(got), total)
	}
	if err := wantExactly(got, "o", total); err != nil {
		return "", fmt.Errorf("output: %w", err)
	}
	if err := adm.committedAtEnd(group, in, 3); err != nil {
		return "", fmt.Errorf("the input offsets: %w", err)
	}
	return fmt.Sprintf("%d records through %d transactions (%d aborted and redone), output exactly once, every input offset committed", total, rounds, aborted), nil
}
