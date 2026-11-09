package main

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"

	lib "github.com/IBM/sarama"
)

type sarama struct{ addr string }

func (sarama) name() string { return "sarama" }

func (s sarama) config() *lib.Config {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	cfg.Producer.Return.Successes = true
	cfg.Producer.RequiredAcks = lib.WaitForAll
	cfg.Consumer.Offsets.Initial = lib.OffsetOldest
	return cfg
}

func (s sarama) produce(_ context.Context, topic string, from, n int) error {
	p, err := lib.NewSyncProducer([]string{s.addr}, s.config())
	if err != nil {
		return err
	}
	defer p.Close()
	for i := from; i < from+n; i++ {
		msg := &lib.ProducerMessage{Topic: topic, Value: lib.StringEncoder(fmt.Sprint(i))}
		if _, _, err := p.SendMessage(msg); err != nil {
			return err
		}
	}
	return nil
}

type saramaHandler struct {
	got  func(string)
	seen *atomic.Int64
	want int64
	done chan struct{}
	once *sync.Once
}

func (saramaHandler) Setup(lib.ConsumerGroupSession) error   { return nil }
func (saramaHandler) Cleanup(lib.ConsumerGroupSession) error { return nil }

func (h saramaHandler) ConsumeClaim(sess lib.ConsumerGroupSession, claim lib.ConsumerGroupClaim) error {
	for m := range claim.Messages() {
		h.got(string(m.Value))
		sess.MarkMessage(m, "")
		if h.seen.Add(1) >= h.want {
			sess.Commit()
			h.once.Do(func() { close(h.done) })
		}
	}
	return nil
}

func (s sarama) consume(ctx context.Context, topic, group string, members, want int, got func(string)) error {
	h := saramaHandler{got: got, seen: new(atomic.Int64), want: int64(want), done: make(chan struct{}), once: new(sync.Once)}
	ctx, stop := context.WithCancel(ctx)
	defer stop()
	var wg sync.WaitGroup
	errs := make(chan error, members)
	groups := make([]lib.ConsumerGroup, members)
	for i := range groups {
		g, err := lib.NewConsumerGroup([]string{s.addr}, group, s.config())
		if err != nil {
			return err
		}
		groups[i] = g
		wg.Add(1)
		go func() {
			defer wg.Done()
			for ctx.Err() == nil {
				if err := g.Consume(ctx, []string{topic}, h); err != nil {
					errs <- err
					return
				}
			}
		}()
	}
	var result error
	select {
	case <-h.done:
	case err := <-errs:
		result = err
	case <-ctx.Done():
		result = ctx.Err()
	}
	stop()
	for _, g := range groups {
		_ = g.Close()
	}
	wg.Wait()
	return result
}
