package main

import (
	"bufio"
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strings"
	"sync"
	"syscall"
	"time"
)

// rdkafka is librdkafka, through kcat: a separate process, so it can also be killed without warning.
type rdkafka struct{ addr string }

func (rdkafka) name() string { return "librdkafka" }

func (r rdkafka) produce(ctx context.Context, topic string, from, n int) error {
	var lines strings.Builder
	for i := from; i < from+n; i++ {
		fmt.Fprintf(&lines, "%d:%d\n", i, i)
	}
	cmd := exec.CommandContext(ctx, "kcat", "-P", "-b", r.addr, "-t", topic, "-K:", "-q")
	cmd.Stdin = strings.NewReader(lines.String())
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("kcat -P: %w: %s", err, bytes.TrimSpace(out))
	}
	return nil
}

func (r rdkafka) join(ctx context.Context, topic, group, assignor string, got func(string), fail func(error)) (member, error) {
	cmd := exec.Command("kcat", "-b", r.addr, "-G", group, "-u", "-q", "-f", "%s\n",
		// Not -o beginning: that starts at offset 0 every time, whatever is committed.
		"-X", "auto.offset.reset=earliest",
		"-X", "partition.assignment.strategy="+assignor,
		// A session that runs out in 6 s (the broker's shortest), so a crashed member is noticed soon.
		"-X", "session.timeout.ms=6000", "-X", "heartbeat.interval.ms=1000",
		"-X", "enable.auto.commit=true", "-X", "auto.commit.interval.ms=500",
		// Last: kcat reads no option after the first topic.
		topic)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.StdoutPipe()
	if err != nil {
		return member{}, err
	}
	if err := cmd.Start(); err != nil {
		return member{}, err
	}
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		sc := bufio.NewScanner(out)
		for sc.Scan() {
			got(sc.Text())
		}
	}()
	done := make(chan struct{})
	go func() { wg.Wait(); _ = cmd.Wait(); close(done) }()
	var mu sync.Mutex
	deliberate := false
	go func() {
		select {
		case <-done:
			mu.Lock()
			d := deliberate
			mu.Unlock()
			if !d {
				fail(fmt.Errorf("kcat -G ended: %s", bytes.TrimSpace(stderr.Bytes())))
			}
		case <-ctx.Done():
			_ = cmd.Process.Kill()
		}
	}()
	var ended sync.Once
	end := func(sig syscall.Signal, wait time.Duration) {
		ended.Do(func() {
			mu.Lock()
			deliberate = true
			mu.Unlock()
			_ = cmd.Process.Signal(sig)
			select {
			case <-done:
			case <-time.After(wait):
				_ = cmd.Process.Kill()
				<-done
			}
		})
	}
	return member{
		// SIGTERM: kcat closes the consumer, which commits and leaves the group.
		stop: func() { end(syscall.SIGTERM, 15*time.Second) },
		// SIGKILL: gone with no word to the broker.
		kill: func() { end(syscall.SIGKILL, 5*time.Second) },
	}, nil
}
