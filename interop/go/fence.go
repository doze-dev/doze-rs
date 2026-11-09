package main

import (
	"context"
	"fmt"
	"time"

	"github.com/twmb/franz-go/pkg/kgo"
)

// fence is a zombie: a transactional producer is writing when another with the same transactional id starts. The broker
// aborts what the first left open and fences it, so the first cannot commit, and a committed reader sees only the second.
type fence struct{}

func (fence) title() string { return "fence-franz-go" }

func (fence) run(addr string) (string, error) {
	ctx, stop := context.WithTimeout(context.Background(), 60*time.Second)
	defer stop()
	topic := fmt.Sprintf("fence-%d", time.Now().UnixNano())
	adm, err := newAdmin(addr)
	if err != nil {
		return "", err
	}
	defer adm.close()
	if err := adm.createTopic(topic, 3); err != nil {
		return "", err
	}
	client := func() (*kgo.Client, error) {
		return kgo.NewClient(kgo.SeedBrokers(addr), kgo.TransactionalID(topic+"-id"), kgo.DefaultProduceTopic(topic), kgo.RecordPartitioner(kgo.RoundRobinPartitioner()))
	}
	write := func(cl *kgo.Client, prefix string, n int) error {
		var recs []*kgo.Record
		for _, v := range values(prefix, n) {
			recs = append(recs, &kgo.Record{Key: []byte(v), Value: []byte(v)})
		}
		return cl.ProduceSync(ctx, recs...).FirstErr()
	}
	old, err := client()
	if err != nil {
		return "", err
	}
	defer old.Close()
	if err := old.BeginTransaction(); err != nil {
		return "", err
	}
	if err := write(old, "z", 5); err != nil {
		return "", fmt.Errorf("the first producer writing: %w", err)
	}
	// A second producer with the same id takes it over: the broker aborts the first's transaction.
	fresh, err := client()
	if err != nil {
		return "", err
	}
	defer fresh.Close()
	if err := fresh.BeginTransaction(); err != nil {
		return "", err
	}
	if err := write(fresh, "b", 5); err != nil {
		return "", fmt.Errorf("the second producer writing: %w", err)
	}
	if err := fresh.EndTransaction(ctx, kgo.TryCommit); err != nil {
		return "", fmt.Errorf("the second producer committing: %w", err)
	}
	// The first finds itself fenced: its commit fails, or comes back as an abort; its records are never committed.
	if err := old.EndTransaction(ctx, kgo.TryCommit); err == nil {
		return "", fmt.Errorf("the fenced producer's commit succeeded")
	}
	committed, err := franzReader{}.read(addr, topic, true)
	if err != nil {
		return "", err
	}
	if err := wantExactly(committed, "b", 5); err != nil {
		return "", fmt.Errorf("reading committed: %w", err)
	}
	everything, err := franzReader{}.read(addr, topic, false)
	if err != nil {
		return "", err
	}
	if err := wantExactly(everything, "z", 5, "b", 5); err != nil {
		return "", fmt.Errorf("reading everything: %w", err)
	}
	return "the second producer's 5 records committed; the fenced one's 5 aborted and its commit refused", nil
}
