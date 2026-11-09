// Command interop drives real Go Kafka clients (sarama, franz-go, kafka-go) against a running broker:
// produce, read through a two-member consumer group, then start a new consumer in the same group
// and check it resumes from what was committed.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"sync"
	"time"
)

// client is what a scenario needs of a library.
type client interface {
	name() string
	// produce sends values "<from>".."<from+n-1>" to the topic.
	produce(ctx context.Context, topic string, from, n int) error
	// consume joins `members` consumers to the group, calls got for each value read, and returns
	// once want values have been seen (offsets committed) or the context ends.
	consume(ctx context.Context, topic, group string, members, want int, got func(string)) error
}

func main() {
	addr := flag.String("broker", "127.0.0.1:9092", "host:port of the broker")
	only := flag.String("only", "", "run one client: sarama, franz-go, kafka-go")
	flag.Parse()
	failed := false
	for _, c := range []client{sarama{*addr}, franz{*addr}, segment{*addr}} {
		if *only != "" && *only != c.name() {
			continue
		}
		if err := scenario(c); err != nil {
			fmt.Printf("FAIL %-8s %v\n", c.name(), err)
			failed = true
			continue
		}
		fmt.Printf("ok   %-8s\n", c.name())
	}
	if failed {
		os.Exit(1)
	}
}

// tally counts the values seen.
type tally struct {
	mu   sync.Mutex
	seen map[string]int
}

func (t *tally) add(v string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.seen[v]++
}

func (t *tally) check(from, n int) error {
	t.mu.Lock()
	defer t.mu.Unlock()
	if len(t.seen) != n {
		return fmt.Errorf("saw %d distinct values, want %d", len(t.seen), n)
	}
	for i := from; i < from+n; i++ {
		if c := t.seen[fmt.Sprint(i)]; c != 1 {
			return fmt.Errorf("value %d seen %d times", i, c)
		}
	}
	return nil
}

func scenario(c client) error {
	ctx, stop := context.WithTimeout(context.Background(), 90*time.Second)
	defer stop()
	topic := fmt.Sprintf("%s-%d", c.name(), time.Now().UnixNano())
	group := topic + "-g"

	const first, second = 300, 50
	// A topic is made by the first request that names it, and a recorded Apache Kafka answers that
	// request "unknown topic" as this broker does. Some clients (kafka-go) give up on that answer, so
	// the first produce is tried again, as an application using them does.
	err := c.produce(ctx, topic, 0, first)
	if err != nil {
		err = c.produce(ctx, topic, 0, first)
	}
	if err != nil {
		return fmt.Errorf("produce: %w", err)
	}
	one := &tally{seen: map[string]int{}}
	if err := c.consume(ctx, topic, group, 2, first, one.add); err != nil {
		return fmt.Errorf("consume with two members: %w", err)
	}
	if err := one.check(0, first); err != nil {
		return fmt.Errorf("two members: %w", err)
	}
	if err := c.produce(ctx, topic, first, second); err != nil {
		return fmt.Errorf("produce again: %w", err)
	}
	two := &tally{seen: map[string]int{}}
	if err := c.consume(ctx, topic, group, 1, second, two.add); err != nil {
		return fmt.Errorf("resume: %w", err)
	}
	if err := two.check(first, second); err != nil {
		return fmt.Errorf("resume from the committed offsets: %w", err)
	}
	return nil
}
