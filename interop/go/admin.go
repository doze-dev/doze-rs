package main

import (
	"context"
	"fmt"
	"time"

	lib "github.com/IBM/sarama"
	"github.com/twmb/franz-go/pkg/kgo"
	"github.com/twmb/franz-go/pkg/kmsg"
)

// admin is sarama's admin client, used by every scenario for the parts that are not under test: making the topic,
// seeing when a group has settled, and where its offsets stand. A group of the new protocol is not described by the
// classic API, so franz-go's client asks ConsumerGroupDescribe first.
type admin struct {
	c  lib.ClusterAdmin
	cl lib.Client
	k  *kgo.Client
}

func newAdmin(addr string) (admin, error) {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	c, err := lib.NewClusterAdmin([]string{addr}, cfg)
	if err != nil {
		return admin{}, err
	}
	cl, err := lib.NewClient([]string{addr}, cfg)
	if err != nil {
		_ = c.Close()
		return admin{}, err
	}
	k, err := kgo.NewClient(kgo.SeedBrokers(addr))
	if err != nil {
		_ = c.Close()
		_ = cl.Close()
		return admin{}, err
	}
	return admin{c, cl, k}, nil
}

func (a admin) close() { _ = a.c.Close(); _ = a.cl.Close(); a.k.Close() }

func (a admin) createTopic(topic string, partitions int32) error {
	return a.c.CreateTopic(topic, &lib.TopicDetail{NumPartitions: partitions, ReplicationFactor: 1}, false)
}

// groupState is the group's state and how many members it lists, whichever protocol it is on now.
// who names the members the group lists, for a failure to say.
func (a admin) who(group string) string {
	req := kmsg.NewPtrConsumerGroupDescribeRequest()
	req.Groups = []string{group}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	resp, err := req.RequestWith(ctx, a.k)
	if err != nil || len(resp.Groups) != 1 {
		return ""
	}
	g := resp.Groups[0]
	out := fmt.Sprintf("[%s epoch %d/%d]", g.State, g.Epoch, g.AssignmentEpoch)
	for _, m := range g.Members {
		n := 0
		for _, t := range m.Assignment.TopicPartitions {
			n += len(t.Partitions)
		}
		out += fmt.Sprintf(" %s@%d(%d)", m.ClientID, m.MemberEpoch, n)
	}
	return out
}

func (a admin) groupState(group string) (state string, members int, err error) {
	req := kmsg.NewPtrConsumerGroupDescribeRequest()
	req.Groups = []string{group}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if resp, err := req.RequestWith(ctx, a.k); err == nil && len(resp.Groups) == 1 && resp.Groups[0].ErrorCode == 0 {
		return resp.Groups[0].State, len(resp.Groups[0].Members), nil
	}
	ds, err := a.c.DescribeConsumerGroups([]string{group})
	if err != nil || len(ds) != 1 {
		return "", 0, err
	}
	return ds[0].State, len(ds[0].Members), nil
}

// waitStable waits until the group is Stable with exactly n members: every one has joined and been given its
// assignment, so what is read next is read under the final split.
func (a admin) waitStable(group string, n int) error {
	deadline := time.Now().Add(40 * time.Second)
	var last string
	for time.Now().Before(deadline) {
		state, members, err := a.groupState(group)
		if err != nil {
			return err
		}
		last = fmt.Sprintf("%s with %d members", state, members)
		if state == "Stable" && members == n {
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("the group never became Stable with %d members (last: %s %s)", n, last, a.who(group))
}

// committedAtEnd checks that the group has committed every record of the topic: each partition's committed offset is
// its end offset.
func (a admin) committedAtEnd(group, topic string, partitions int32) error {
	all := make([]int32, partitions)
	for p := range all {
		all[p] = int32(p)
	}
	res, err := a.c.ListConsumerGroupOffsets(group, map[string][]int32{topic: all})
	if err != nil {
		return err
	}
	for _, p := range all {
		end, err := a.cl.GetOffset(topic, p, lib.OffsetNewest)
		if err != nil {
			return err
		}
		block := res.GetBlock(topic, p)
		if block == nil || block.Offset != end {
			got := int64(-1)
			if block != nil {
				got = block.Offset
			}
			return fmt.Errorf("partition %d: committed %d, ends at %d", p, got, end)
		}
	}
	return nil
}

// waitMembers waits until the group lists exactly n members, in whatever state, and says how long it took.
func (a admin) waitMembers(group string, n int) (time.Duration, error) {
	start := time.Now()
	for time.Since(start) < 40*time.Second {
		_, members, err := a.groupState(group)
		if err != nil {
			return 0, err
		}
		if members == n {
			return time.Since(start), nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return 0, fmt.Errorf("the group never listed %d members", n)
}

// createTopicWith makes a topic with these configs (a compacted topic with small segments, say).
func (a admin) createTopicWith(topic string, partitions int32, configs map[string]*string) error {
	return a.c.CreateTopic(topic, &lib.TopicDetail{NumPartitions: partitions, ReplicationFactor: 1, ConfigEntries: configs}, false)
}
