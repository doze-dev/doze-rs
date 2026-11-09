package main

import (
	"context"
	"fmt"
	"sync"
	"time"

	lib "github.com/segmentio/kafka-go"
)

type segment struct{ addr string }

func (segment) name() string { return "kafka-go" }

func (s segment) produce(ctx context.Context, topic string, from, n int) error {
	w := &lib.Writer{
		Addr:         lib.TCP(s.addr),
		Topic:        topic,
		RequiredAcks: lib.RequireAll,
		Balancer:     &lib.Hash{},
		BatchTimeout: 5 * time.Millisecond,
	}
	defer w.Close()
	msgs := make([]lib.Message, 0, n)
	for i := from; i < from+n; i++ {
		msgs = append(msgs, lib.Message{Key: []byte(fmt.Sprint(i)), Value: []byte(fmt.Sprint(i))})
	}
	return w.WriteMessages(ctx, msgs...)
}

func (s segment) join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (func(), error) {
	balancer := lib.GroupBalancer(lib.RangeGroupBalancer{})
	if assignor == "roundrobin" {
		balancer = lib.RoundRobinGroupBalancer{}
	}
	r := lib.NewReader(lib.ReaderConfig{
		Brokers:        []string{s.addr},
		GroupID:        group,
		Topic:          topic,
		MinBytes:       1,
		MaxBytes:       1 << 20,
		MaxWait:        200 * time.Millisecond,
		GroupBalancers: []lib.GroupBalancer{balancer},
		// A group of its own starts from the first record.
		StartOffset: lib.FirstOffset,
	})
	ctx, cancel := context.WithCancel(ctx)
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for ctx.Err() == nil {
			m, err := r.FetchMessage(ctx)
			if err != nil {
				if ctx.Err() == nil {
					fail(err)
				}
				return
			}
			got(string(m.Value))
			if err := r.CommitMessages(ctx, m); err != nil && ctx.Err() == nil {
				fail(err)
				return
			}
		}
	}()
	return func() { cancel(); wg.Wait(); _ = r.Close() }, nil
}
