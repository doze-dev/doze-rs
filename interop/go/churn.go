package main

import (
	"context"
	"fmt"
	"sync/atomic"
	"time"
)

// churn is a group under load whose members come and go: while records are produced all along, one member
// leaves, one is killed (no LeaveGroup; the broker finds out when its session runs out), and a new one joins.
// Every record must still be read, and everything must be committed at the end.
type churn struct {
	name    string
	clients []client
	// leave is the library whose member leaves, kill the one whose member is killed.
	leave, kill string
}

func (c churn) title() string { return c.name }

func (c churn) run(addr string) (string, error) {
	ctx, stop := context.WithTimeout(context.Background(), 120*time.Second)
	defer stop()
	topic := fmt.Sprintf("%s-%d", c.name, time.Now().UnixNano())
	group := topic + "-g"
	adm, err := newAdmin(addr)
	if err != nil {
		return "", err
	}
	defer adm.close()
	if err := adm.createTopic(topic, partitions); err != nil {
		return "", err
	}
	byName := map[string]client{}
	for _, m := range c.clients {
		byName[m.name()] = m
	}
	s := scenario{assignor: "range"}
	tl := newTally()
	g, err := s.start(ctx, adm, topic, group, c.clients, tl)
	if err != nil {
		return "", err
	}
	defer g.stop()
	if err := adm.waitStable(group, len(c.clients)); err != nil {
		return "", fmt.Errorf("at the start: %w", err)
	}

	// The load: 40 batches of 50 records, a quarter of a second apart (ten seconds).
	const batches, size = 40, 50
	var produced atomic.Int64
	produceDone := make(chan error, 1)
	go func() {
		p := byName["sarama"]
		for b := 0; b < batches; b++ {
			if err := p.produce(ctx, topic, b*size, size); err != nil {
				produceDone <- fmt.Errorf("batch %d: %w", b, err)
				return
			}
			produced.Add(size)
			time.Sleep(250 * time.Millisecond)
		}
		produceDone <- nil
	}()
	at := func(n int64) error { // wait until n records have been produced
		for produced.Load() < n {
			select {
			case err := <-g.failed:
				return err
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(10 * time.Millisecond):
			}
		}
		return nil
	}
	left := func(name string) member { // the live member of that library
		for i, m := range g.members {
			if m.name() == name {
				return g.live[i]
			}
		}
		return member{}
	}

	// One leaves, and the rest settle.
	if err := at(300); err != nil {
		return "", err
	}
	left(c.leave).stop()
	if err := adm.waitStable(group, len(c.clients)-1); err != nil {
		return "", fmt.Errorf("after franz-go left: %w", err)
	}
	// One is killed. The broker holds its seat until the session runs out, then rebalances.
	if err := at(900); err != nil {
		return "", err
	}
	left(c.kill).kill()
	// Its session is 6 s and it last heartbeated within a second before the kill: the seat goes between 4 s and
	// 7 s after it (the broker's timer, a little slack for polling).
	expired, err := adm.waitMembers(group, len(c.clients)-2)
	if err != nil {
		return "", fmt.Errorf("after librdkafka was killed: %w", err)
	}
	if expired < 4*time.Second || expired > 7500*time.Millisecond {
		return "", fmt.Errorf("the killed member's seat went after %v, want about 6 s", expired)
	}
	settled := time.Now()
	if err := adm.waitStable(group, len(c.clients)-2); err != nil {
		return "", fmt.Errorf("after librdkafka was killed: %w", err)
	}
	regrouped := time.Since(settled)
	// One joins, while the load goes on.
	if _, err := g.add(ctx, byName["sarama"], topic, group, "range", tl); err != nil {
		return "", err
	}
	if err := adm.waitStable(group, len(c.clients)-1); err != nil {
		return "", fmt.Errorf("after a member joined: %w", err)
	}

	if err := <-produceDone; err != nil {
		return "", err
	}
	if err := g.until(ctx, batches*size, tl); err != nil {
		return "", err
	}
	g.stop()
	extra, err := tl.check(0, batches*size, false)
	if err != nil {
		return "", err
	}
	if err := adm.committedAtEnd(group, topic, partitions); err != nil {
		return "", fmt.Errorf("after everyone left: %w", err)
	}
	return fmt.Sprintf("%d records, killed member's seat went after %.1fs and the rest were regrouped %.1fs later, %d delivered twice, all committed", batches*size, expired.Seconds(), regrouped.Seconds(), extra), nil
}
