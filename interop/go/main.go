// Command interop drives real Go Kafka clients (sarama, franz-go, kafka-go) against a running broker:
// produce, read through consumer groups (of one library, and of all three together), then start a new
// consumer in the same group and check it resumes from what was committed.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"sync"
	"sync/atomic"
	"time"
)

// client is what a scenario needs of a library.
type client interface {
	name() string
	// produce sends values "<from>".."<from+n-1>" to the topic.
	produce(ctx context.Context, topic string, from, n int) error
	// join starts one member of the group using the named assignor ("range" or "roundrobin") and returns
	// at once. got is called for each value read; fail for an error that ends the member. stop leaves
	// the group, which commits what was read.
	join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (stop func(), err error)
}

func main() {
	addr := flag.String("broker", "127.0.0.1:9092", "host:port of the broker")
	only := flag.String("only", "", "run one scenario: sarama, franz-go, kafka-go, mixed")
	flag.Parse()
	all := []client{sarama{*addr}, franz{*addr}, segment{*addr}}
	var scenarios []scenario
	for _, c := range all {
		scenarios = append(scenarios, scenario{c.name(), []client{c, c}, "range", true})
	}
	scenarios = append(scenarios,
		scenario{"franz-sarama", []client{all[1], all[0]}, "range", false},
		scenario{"franz-kafka-go", []client{all[1], all[2]}, "range", false},
		scenario{"sarama-franz", []client{all[0], all[1]}, "range", false},
		scenario{"mixed", all, "range", false},
		scenario{"mixed-rr", all, "roundrobin", false})
	failed := false
	for _, s := range scenarios {
		if *only != "" && *only != s.name {
			continue
		}
		note, err := s.run(*addr)
		if err != nil {
			fmt.Printf("FAIL %-9s %v\n", s.name, err)
			failed = true
			continue
		}
		fmt.Printf("ok   %-9s %s\n", s.name, note)
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

func newTally() *tally { return &tally{seen: map[string]int{}} }

func (t *tally) add(v string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.seen[v]++
}

func (t *tally) distinct() int {
	t.mu.Lock()
	defer t.mu.Unlock()
	return len(t.seen)
}

// check wants every value in from..from+n-1 and no other; once each when exact, else at least once.
// It returns how many extra deliveries there were (a rebalance may hand a record out again).
func (t *tally) check(from, n int, exact bool) (extra int, err error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if len(t.seen) != n {
		return 0, fmt.Errorf("saw %d distinct values, want %d", len(t.seen), n)
	}
	for i := from; i < from+n; i++ {
		c := t.seen[fmt.Sprint(i)]
		if c == 0 || (exact && c != 1) {
			return 0, fmt.Errorf("value %d seen %d times", i, c)
		}
		extra += c - 1
	}
	return extra, nil
}

// scenario is one group: its members, the assignor they share, and whether each record must arrive once.
type scenario struct {
	name     string
	members  []client
	assignor string
	exact    bool
}

// running is a group's members, started.
type running struct {
	members []client
	reads   []int64
	failed  chan error
	stops   []func()
}

func (r *running) stop() {
	for _, stop := range r.stops {
		stop()
	}
	r.stops = nil
}

// start joins each of the members to the group; it returns once they have all been started, not settled.
func (s scenario) start(ctx context.Context, topic, group string, members []client, t *tally) (*running, error) {
	r := &running{members: members, reads: make([]int64, len(members)), failed: make(chan error, len(members))}
	for i, m := range members {
		count := &r.reads[i]
		got := func(v string) { atomic.AddInt64(count, 1); t.add(v) }
		fail := func(err error) { r.failed <- fmt.Errorf("%s: %w", m.name(), err) }
		stop, err := m.join(ctx, topic, group, s.assignor, got, fail)
		if err != nil {
			r.stop()
			return nil, fmt.Errorf("%s: %w", m.name(), err)
		}
		r.stops = append(r.stops, stop)
	}
	return r, nil
}

// until waits for want distinct values.
func (r *running) until(ctx context.Context, want int, t *tally) error {
	tick := time.NewTicker(20 * time.Millisecond)
	defer tick.Stop()
	for t.distinct() < want {
		select {
		case err := <-r.failed:
			return err
		case <-ctx.Done():
			return fmt.Errorf("saw %d of %d values: %w", t.distinct(), want, ctx.Err())
		case <-tick.C:
		}
	}
	return nil
}

func (s scenario) run(addr string) (string, error) {
	ctx, stop := context.WithTimeout(context.Background(), 90*time.Second)
	defer stop()
	topic := fmt.Sprintf("%s-%d", s.name, time.Now().UnixNano())
	group := topic + "-g"
	producer := s.members[0]
	adm, err := newAdmin(addr)
	if err != nil {
		return "", fmt.Errorf("admin: %w", err)
	}
	if err := adm.createTopic(topic, 3); err != nil {
		return "", fmt.Errorf("create topic: %w", err)
	}

	// Every member joins and settles first, so that the records are read under the final split.
	const first, second = 300, 50
	one := newTally()
	group1, err := s.start(ctx, topic, group, s.members, one)
	if err != nil {
		return "", err
	}
	defer group1.stop()
	if err := adm.waitStable(group, len(s.members)); err != nil {
		return "", err
	}
	if err := producer.produce(ctx, topic, 0, first); err != nil {
		return "", fmt.Errorf("produce: %w", err)
	}
	if err := group1.until(ctx, first, one); err != nil {
		return "", fmt.Errorf("group of %d: %w", len(s.members), err)
	}
	group1.stop()
	var split []string
	for i, m := range s.members {
		if group1.reads[i] == 0 {
			return "", fmt.Errorf("member %d (%s) read nothing: %v", i, m.name(), group1.reads)
		}
		split = append(split, fmt.Sprintf("%s=%d", m.name(), group1.reads[i]))
	}
	extra, err := one.check(0, first, s.exact)
	if err != nil {
		return "", fmt.Errorf("group of %d: %w", len(s.members), err)
	}

	// A new member of the group, of the library that did not produce, starts from the commit.
	if err := producer.produce(ctx, topic, first, second); err != nil {
		return "", fmt.Errorf("produce again: %w", err)
	}
	two := newTally()
	group2, err := s.start(ctx, topic, group, []client{s.members[len(s.members)-1]}, two)
	if err != nil {
		return "", err
	}
	defer group2.stop()
	if err := group2.until(ctx, second, two); err != nil {
		return "", fmt.Errorf("resume: %w", err)
	}
	if _, err := two.check(first, second, true); err != nil {
		return "", fmt.Errorf("resume from the committed offsets: %w", err)
	}
	return fmt.Sprintf("%v; %d redelivered; %d read after the resume", split, extra, second), nil
}
