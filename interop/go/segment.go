package main

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	lib "github.com/segmentio/kafka-go"
)

type segment struct{ addr string }

func (segment) name() string { return "kafka-go" }

func (s segment) produce(ctx context.Context, topic string, from, n int) error {
	w := &lib.Writer{
		Addr:                   lib.TCP(s.addr),
		Topic:                  topic,
		RequiredAcks:           lib.RequireAll,
		AllowAutoTopicCreation: true,
		BatchTimeout:           5 * time.Millisecond,
	}
	defer w.Close()
	msgs := make([]lib.Message, 0, n)
	for i := from; i < from+n; i++ {
		msgs = append(msgs, lib.Message{Value: []byte(fmt.Sprint(i))})
	}
	return w.WriteMessages(ctx, msgs...)
}

func (s segment) consume(ctx context.Context, topic, group string, members, want int, got func(string)) error {
	ctx, stop := context.WithCancel(ctx)
	defer stop()
	var seen atomic.Int64
	done := make(chan struct{})
	var once sync.Once
	var wg sync.WaitGroup
	errs := make(chan error, members)
	readers := make([]*lib.Reader, members)
	for i := range readers {
		r := lib.NewReader(lib.ReaderConfig{
			Brokers:  []string{s.addr},
			GroupID:  group,
			Topic:    topic,
			MinBytes: 1,
			MaxBytes: 1 << 20,
			MaxWait:  200 * time.Millisecond,
			// A group of its own starts from the first record.
			StartOffset: lib.FirstOffset,
		})
		readers[i] = r
		wg.Add(1)
		go func() {
			defer wg.Done()
			for ctx.Err() == nil {
				m, err := r.FetchMessage(ctx)
				if err != nil {
					if ctx.Err() == nil {
						errs <- err
					}
					return
				}
				got(string(m.Value))
				if err := r.CommitMessages(ctx, m); err != nil && ctx.Err() == nil {
					errs <- err
					return
				}
				if seen.Add(1) >= int64(want) {
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
	for _, r := range readers {
		_ = r.Close()
	}
	wg.Wait()
	return result
}
