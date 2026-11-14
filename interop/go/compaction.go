package main

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"time"

	lib "github.com/IBM/sarama"
	"github.com/twmb/franz-go/pkg/kgo"
)

// compaction writes keyed records to a compacted topic with small segments, waits for the broker to clean it, and reads
// it from the start with three libraries: each must see the latest value of every key, in offset order, and the topic must
// still be read from the middle of what was removed.
type compaction struct{}

func (compaction) title() string { return "compaction" }

// record is what a consumer saw: its offset, key and value.
type record struct {
	offset     int64
	key, value string
}

func (compaction) run(addr string) (string, error) {
	const total, keys = 600, 10
	ctx, stop := context.WithTimeout(context.Background(), 120*time.Second)
	defer stop()
	topic := fmt.Sprintf("compact-%d", time.Now().UnixNano())
	adm, err := newAdmin(addr)
	if err != nil {
		return "", err
	}
	defer adm.close()
	str := func(s string) *string { return &s }
	err = adm.createTopicWith(topic, 1, map[string]*string{
		"cleanup.policy":            str("compact"),
		"segment.bytes":             str("1024"),
		"min.cleanable.dirty.ratio": str("0.01"),
		"delete.retention.ms":       str("1"),
	})
	if err != nil {
		return "", fmt.Errorf("creating the topic: %w", err)
	}
	prod, err := kgo.NewClient(kgo.SeedBrokers(addr), kgo.RequiredAcks(kgo.AllISRAcks()), kgo.DefaultProduceTopic(topic))
	if err != nil {
		return "", err
	}
	defer prod.Close()
	latest := map[string]string{}
	for i := 0; i < total; i++ {
		k, v := fmt.Sprintf("k%d", i%keys), fmt.Sprintf("v%d", i)
		latest[k] = v
		if res := prod.ProduceSync(ctx, &kgo.Record{Key: []byte(k), Value: []byte(v)}); res.FirstErr() != nil {
			return "", fmt.Errorf("producing: %w", res.FirstErr())
		}
	}
	// The broker cleans every half second here (run.sh); wait until the topic has got much shorter than it was written.
	var seen []record
	deadline := time.Now().Add(60 * time.Second)
	for {
		if seen, err = readFranz(ctx, addr, topic, 0, total); err != nil {
			return "", err
		}
		if len(seen) < total/2 {
			break
		}
		if time.Now().After(deadline) {
			return "", fmt.Errorf("the topic still holds %d of %d records", len(seen), total)
		}
		time.Sleep(500 * time.Millisecond)
	}
	if err := checkLatest("franz-go", seen, latest, total); err != nil {
		return "", err
	}
	viaSarama, err := readSarama(addr, topic, lib.OffsetOldest, total)
	if err != nil {
		return "", err
	}
	if err := checkLatest("sarama", viaSarama, latest, total); err != nil {
		return "", err
	}
	viaKcat, err := readKcat(ctx, addr, topic)
	if err != nil {
		return "", err
	}
	if err := checkLatest("librdkafka", viaKcat, latest, total); err != nil {
		return "", err
	}
	// From an offset that was cleaned away: the first record at or after it, never an error.
	mid := int64(total / 3)
	if got, err := readFranz(ctx, addr, topic, mid, total); err != nil || len(got) == 0 || got[0].offset < mid {
		return "", fmt.Errorf("franz-go from the middle (%d): %v %v", mid, got, err)
	}
	if got, err := readSarama(addr, topic, mid, total); err != nil || len(got) == 0 || got[0].offset < mid {
		return "", fmt.Errorf("sarama from the middle (%d): %v %v", mid, got, err)
	}
	return fmt.Sprintf("%d of %d records left, latest of %d keys, three libraries agree", len(seen), total, keys), nil
}

// checkLatest wants the offsets in order and, for every key, its latest value as the last record of the key seen.
func checkLatest(who string, seen []record, latest map[string]string, total int) error {
	last := map[string]string{}
	for i, r := range seen {
		if i > 0 && r.offset <= seen[i-1].offset {
			return fmt.Errorf("%s: offsets out of order at %d", who, r.offset)
		}
		last[r.key] = r.value
	}
	for k, v := range latest {
		if last[k] != v {
			return fmt.Errorf("%s: key %s ends at %q, want %q", who, k, last[k], v)
		}
	}
	if len(seen) >= total {
		return fmt.Errorf("%s: nothing was removed", who)
	}
	return nil
}

// readFranz reads the partition from `from` to its end (offset `total`), with franz-go.
func readFranz(ctx context.Context, addr, topic string, from int64, total int) ([]record, error) {
	cl, err := kgo.NewClient(kgo.SeedBrokers(addr),
		kgo.ConsumePartitions(map[string]map[int32]kgo.Offset{topic: {0: kgo.NewOffset().At(from)}}))
	if err != nil {
		return nil, err
	}
	defer cl.Close()
	var out []record
	for {
		pctx, cancel := context.WithTimeout(ctx, 3*time.Second)
		fetches := cl.PollFetches(pctx)
		cancel()
		if err := fetches.Err0(); err != nil && ctx.Err() == nil && pctx.Err() == nil {
			return nil, fmt.Errorf("franz-go fetching: %w", err)
		}
		n := 0
		fetches.EachRecord(func(r *kgo.Record) {
			n++
			out = append(out, record{r.Offset, string(r.Key), string(r.Value)})
		})
		if n == 0 || (len(out) > 0 && out[len(out)-1].offset >= int64(total-1)) {
			return out, nil
		}
	}
}

// readSarama reads the partition from `from` to its end, with sarama's partition consumer.
func readSarama(addr, topic string, from int64, total int) ([]record, error) {
	cfg := saramaConfig()
	c, err := lib.NewConsumer([]string{addr}, cfg)
	if err != nil {
		return nil, err
	}
	defer c.Close()
	pc, err := c.ConsumePartition(topic, 0, from)
	if err != nil {
		return nil, err
	}
	defer pc.Close()
	var out []record
	for {
		select {
		case m := <-pc.Messages():
			out = append(out, record{m.Offset, string(m.Key), string(m.Value)})
			if m.Offset >= int64(total-1) {
				return out, nil
			}
		case err := <-pc.Errors():
			return nil, fmt.Errorf("sarama: %w", err)
		case <-time.After(5 * time.Second):
			return out, nil
		}
	}
}

// readKcat reads the partition from the start to its end with librdkafka (kcat), one line per record.
func readKcat(ctx context.Context, addr, topic string) ([]record, error) {
	cmd := exec.CommandContext(ctx, "kcat", "-C", "-b", addr, "-t", topic, "-p", "0", "-o", "beginning", "-e", "-q", "-f", "%o\t%k\t%s\n")
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("kcat -C: %w: %s", err, strings.TrimSpace(stderr.String()))
	}
	var recs []record
	for _, line := range strings.Split(strings.TrimSpace(string(out)), "\n") {
		parts := strings.Split(line, "\t")
		if len(parts) != 3 {
			return nil, fmt.Errorf("kcat printed %q", line)
		}
		o, err := strconv.ParseInt(parts[0], 10, 64)
		if err != nil {
			return nil, err
		}
		recs = append(recs, record{o, parts[1], parts[2]})
	}
	return recs, nil
}
