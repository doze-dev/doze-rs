package main

import (
	"context"
	"fmt"
	"sync"

	lib "github.com/IBM/sarama"
)

type sarama struct{ addr string }

func (sarama) name() string { return "sarama" }

func (s sarama) config(assignor string) *lib.Config {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	cfg.Producer.Return.Successes = true
	cfg.Producer.RequiredAcks = lib.WaitForAll
	cfg.Consumer.Offsets.Initial = lib.OffsetOldest
	if assignor == "roundrobin" {
		cfg.Consumer.Group.Rebalance.GroupStrategies = []lib.BalanceStrategy{lib.NewBalanceStrategyRoundRobin()}
	} else {
		cfg.Consumer.Group.Rebalance.GroupStrategies = []lib.BalanceStrategy{lib.NewBalanceStrategyRange()}
	}
	return cfg
}

func (s sarama) produce(_ context.Context, topic string, from, n int) error {
	p, err := lib.NewSyncProducer([]string{s.addr}, s.config("range"))
	if err != nil {
		return err
	}
	defer p.Close()
	for i := from; i < from+n; i++ {
		msg := &lib.ProducerMessage{Topic: topic, Key: lib.StringEncoder(fmt.Sprint(i)), Value: lib.StringEncoder(fmt.Sprint(i))}
		if _, _, err := p.SendMessage(msg); err != nil {
			return err
		}
	}
	return nil
}

type saramaHandler struct{ got func(string) }

func (saramaHandler) Setup(lib.ConsumerGroupSession) error   { return nil }
func (saramaHandler) Cleanup(lib.ConsumerGroupSession) error { return nil }

func (h saramaHandler) ConsumeClaim(sess lib.ConsumerGroupSession, claim lib.ConsumerGroupClaim) error {
	for m := range claim.Messages() {
		h.got(string(m.Value))
		sess.MarkMessage(m, "")
	}
	return nil
}

func (s sarama) join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (func(), error) {
	g, err := lib.NewConsumerGroup([]string{s.addr}, group, s.config(assignor))
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for ctx.Err() == nil {
			if err := g.Consume(ctx, []string{topic}, saramaHandler{got}); err != nil && ctx.Err() == nil {
				fail(err)
				return
			}
		}
	}()
	// Closing the group commits the offsets marked.
	return func() { cancel(); wg.Wait(); _ = g.Close() }, nil
}
