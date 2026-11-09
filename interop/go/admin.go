package main

import (
	"fmt"
	"time"

	lib "github.com/IBM/sarama"
)

// admin is sarama's admin client, used by every scenario for the parts that are not under test:
// making the topic, seeing when a group has settled, and where its offsets stand.
type admin struct {
	c  lib.ClusterAdmin
	cl lib.Client
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
	return admin{c, cl}, nil
}

func (a admin) close() { _ = a.c.Close(); _ = a.cl.Close() }

func (a admin) createTopic(topic string, partitions int32) error {
	return a.c.CreateTopic(topic, &lib.TopicDetail{NumPartitions: partitions, ReplicationFactor: 1}, false)
}

// waitStable waits until the group is Stable with exactly n members: every one has joined and been given
// its assignment, so what is read next is read under the final split.
func (a admin) waitStable(group string, n int) error {
	deadline := time.Now().Add(40 * time.Second)
	var last string
	for time.Now().Before(deadline) {
		ds, err := a.c.DescribeConsumerGroups([]string{group})
		if err != nil {
			return err
		}
		if len(ds) == 1 {
			last = fmt.Sprintf("%s with %d members", ds[0].State, len(ds[0].Members))
			if ds[0].State == "Stable" && len(ds[0].Members) == n {
				return nil
			}
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("the group never became Stable with %d members (last: %s)", n, last)
}

// committedAtEnd checks that the group has committed every record of the topic: each partition's committed
// offset is its end offset.
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
		ds, err := a.c.DescribeConsumerGroups([]string{group})
		if err != nil {
			return 0, err
		}
		if len(ds) == 1 && len(ds[0].Members) == n {
			return time.Since(start), nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return 0, fmt.Errorf("the group never listed %d members", n)
}
