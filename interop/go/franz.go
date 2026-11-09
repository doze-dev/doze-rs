package main

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"

	"github.com/twmb/franz-go/pkg/kgo"
)

type franz struct{ addr string }

func (franz) name() string { return "franz-go" }

func (f franz) produce(ctx context.Context, topic string, from, n int) error {
	cl, err := kgo.NewClient(kgo.SeedBrokers(f.addr), kgo.DefaultProduceTopic(topic), kgo.AllowAutoTopicCreation())
	if err != nil {
		return err
	}
	defer cl.Close()
	recs := make([]*kgo.Record, 0, n)
	for i := from; i < from+n; i++ {
		recs = append(recs, &kgo.Record{Value: []byte(fmt.Sprint(i))})
	}
	return cl.ProduceSync(ctx, recs...).FirstErr()
}

func (f franz) consume(ctx context.Context, topic, group string, members, want int, got func(string)) error {
	ctx, stop := context.WithCancel(ctx)
	defer stop()
	var seen atomic.Int64
	done := make(chan struct{})
	var once sync.Once
	clients := make([]*kgo.Client, members)
	for i := range clients {
		cl, err := kgo.NewClient(
			kgo.SeedBrokers(f.addr),
			kgo.ConsumerGroup(group),
			kgo.ConsumeTopics(topic),
			kgo.ConsumeResetOffset(kgo.NewOffset().AtStart()),
			kgo.DisableAutoCommit(),
		)
		if err != nil {
			return err
		}
		clients[i] = cl
	}
	var wg sync.WaitGroup
	errs := make(chan error, members)
	for _, cl := range clients {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for ctx.Err() == nil {
				fetches := cl.PollFetches(ctx)
				if err := fetches.Err0(); err != nil && ctx.Err() == nil {
					errs <- err
					return
				}
				fetches.EachRecord(func(r *kgo.Record) {
					got(string(r.Value))
					seen.Add(1)
				})
				if err := cl.CommitUncommittedOffsets(ctx); err != nil && ctx.Err() == nil {
					errs <- err
					return
				}
				if seen.Load() >= int64(want) {
					once.Do(func() { close(done) })
				}
			}
		}()
	}
	var result error
	select {
	case <-done:
	case err := <-errs:
		result = err
	case <-ctx.Done():
		result = ctx.Err()
	}
	stop()
	wg.Wait()
	for _, cl := range clients {
		cl.Close()
	}
	return result
}
