package main

import (
	"context"
	"fmt"
	"os"
	"sync"

	"github.com/twmb/franz-go/pkg/kgo"
)

type franz struct {
	addr   string
	kip848 bool
}

func (f franz) name() string {
	if f.kip848 {
		return "franz-go-848"
	}
	return "franz-go"
}

func (f franz) produce(ctx context.Context, topic string, from, n int) error {
	cl, err := kgo.NewClient(kgo.SeedBrokers(f.addr), kgo.DefaultProduceTopic(topic), kgo.AllowAutoTopicCreation())
	if err != nil {
		return err
	}
	defer cl.Close()
	recs := make([]*kgo.Record, 0, n)
	for i := from; i < from+n; i++ {
		recs = append(recs, &kgo.Record{Key: []byte(fmt.Sprint(i)), Value: []byte(fmt.Sprint(i))})
	}
	return cl.ProduceSync(ctx, recs...).FirstErr()
}

func (f franz) join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (member, error) {
	balancer := kgo.RangeBalancer()
	if assignor == "roundrobin" {
		balancer = kgo.RoundRobinBalancer()
	}
	opts := []kgo.Opt{}
	if os.Getenv("FRANZ_LOG") != "" {
		opts = append(opts, kgo.WithLogger(kgo.BasicLogger(os.Stderr, kgo.LogLevelDebug, nil)))
	}
	if f.kip848 {
		// The broker assigns (it needs the range or the sticky balancer to be named, and Kafka 4).
		opts = append(opts, kgo.ServerSideBalancer(), kgo.Balancers(kgo.RangeBalancer()))
	} else {
		opts = append(opts, kgo.Balancers(balancer))
	}
	cl, err := kgo.NewClient(append(opts,
		kgo.SeedBrokers(f.addr),
		kgo.ConsumerGroup(group),
		kgo.ConsumeTopics(topic),
		kgo.ConsumeResetOffset(kgo.NewOffset().AtStart()),
		kgo.DisableAutoCommit(),
	)...)
	if err != nil {
		return member{}, err
	}
	ctx, cancel := context.WithCancel(ctx)
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for ctx.Err() == nil {
			fetches := cl.PollFetches(ctx)
			if err := fetches.Err0(); err != nil && ctx.Err() == nil {
				fail(err)
				return
			}
			fetches.EachRecord(func(r *kgo.Record) { got(string(r.Value)) })
			if err := cl.CommitUncommittedOffsets(ctx); err != nil && ctx.Err() == nil {
				fail(err)
				return
			}
		}
	}()
	return member{stop: func() { cancel(); wg.Wait(); cl.Close() }, late: f.kip848}, nil
}
