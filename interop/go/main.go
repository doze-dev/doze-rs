// Command interop drives real Kafka clients (sarama, franz-go, kafka-go, librdkafka through kcat) against a
// running broker: consumer groups of one library and of several together, a new consumer resuming from what was
// committed, and a group whose members leave, crash and join while records flow.
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

// member is one running consumer of a group.
type member struct {
	// stop leaves the group, which commits what was read.
	stop func()
	// kill is an end with no word to the broker, for a client that is a process of its own (nil if not).
	kill func()
}

// client is what a scenario needs of a library.
type client interface {
	name() string
	// produce sends the records "<from>".."<from+n-1>", each with itself as key and as value.
	produce(ctx context.Context, topic string, from, n int) error
	// join starts one member of the group using the named assignor ("range" or "roundrobin") and returns
	// at once. got is called for each value read; fail for an error that ends the member.
	join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (member, error)
}

// partitions is the size of every topic: at least one for each member of the largest group.
const partitions = 4

func main() {
	addr := flag.String("broker", "127.0.0.1:9092", "host:port of the broker")
	only := flag.String("only", "", "run one scenario by name")
	flag.Parse()
	sar, fra, seg, rdk := sarama{*addr}, franz{*addr}, segment{*addr}, rdkafka{*addr}
	all := []client{sar, fra, seg, rdk}
	var scenarios []runner
	for _, c := range all {
		scenarios = append(scenarios, scenario{c.name(), []client{c, c}, "range", true})
	}
	scenarios = append(scenarios,
		scenario{"franz-sarama", []client{fra, sar}, "range", false},
		scenario{"franz-kafka-go", []client{fra, seg}, "range", false},
		scenario{"sarama-franz", []client{sar, fra}, "range", false},
		scenario{"librdkafka-franz", []client{rdk, fra}, "range", false},
		scenario{"franz-librdkafka", []client{fra, rdk}, "range", false},
		scenario{"mixed", all, "range", false},
		scenario{"mixed-rr", all, "roundrobin", false},
		churn{all})
	failed := false
	for _, s := range scenarios {
		if *only != "" && *only != s.title() {
			continue
		}
		start := time.Now()
		note, err := s.run(*addr)
		if err != nil {
			fmt.Printf("FAIL %-17s %v\n", s.title(), err)
			failed = true
			continue
		}
		fmt.Printf("ok   %-17s %4.1fs  %s\n", s.title(), time.Since(start).Seconds(), note)
	}
	if failed {
		os.Exit(1)
	}
}

type runner interface {
	title() string
	run(addr string) (string, error)
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

// running is a group's members, started.
type running struct {
	members []client
	live    []member
	reads   []*int64
	failed  chan error
}

func (r *running) stop() {
	for i := range r.live {
		if r.live[i].stop != nil {
			r.live[i].stop()
		}
	}
}

// add joins one more member to the group, returning its place in r.
func (r *running) add(ctx context.Context, m client, topic, group, assignor string, t *tally) (int, error) {
	r.members = append(r.members, m)
	count := new(int64)
	r.reads = append(r.reads, count)
	i := len(r.members) - 1
	got := func(v string) { atomic.AddInt64(count, 1); t.add(v) }
	fail := func(err error) { r.failed <- fmt.Errorf("%s: %w", m.name(), err) }
	live, err := m.join(ctx, topic, group, assignor, got, fail)
	if err != nil {
		return 0, fmt.Errorf("%s: %w", m.name(), err)
	}
	r.live = append(r.live, live)
	return i, nil
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

// scenario is one group: its members, the assignor they share, and whether each record must arrive once.
type scenario struct {
	name     string
	members  []client
	assignor string
	exact    bool
}

func (s scenario) title() string { return s.name }

// start joins each of the members to the group; it returns once they have all been started, not settled.
func (s scenario) start(ctx context.Context, topic, group string, members []client, t *tally) (*running, error) {
	r := &running{failed: make(chan error, 16)}
	for _, m := range members {
		if _, err := r.add(ctx, m, topic, group, s.assignor, t); err != nil {
			r.stop()
			return nil, err
		}
	}
	return r, nil
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
	defer adm.close()
	if err := adm.createTopic(topic, partitions); err != nil {
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
		n := atomic.LoadInt64(group1.reads[i])
		if n == 0 {
			return "", fmt.Errorf("member %d (%s) read nothing", i, m.name())
		}
		split = append(split, fmt.Sprintf("%s=%d", m.name(), n))
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
