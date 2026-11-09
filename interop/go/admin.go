package main

import (
	"fmt"
	"time"

	lib "github.com/IBM/sarama"
)

// admin is sarama's admin client, used by every scenario for the parts that are not under test:
// making the topic, and seeing when a group has settled.
type admin struct{ c lib.ClusterAdmin }

func newAdmin(addr string) (admin, error) {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	c, err := lib.NewClusterAdmin([]string{addr}, cfg)
	return admin{c}, err
}

func (a admin) createTopic(topic string, partitions int32) error {
	return a.c.CreateTopic(topic, &lib.TopicDetail{NumPartitions: partitions, ReplicationFactor: 1}, false)
}

// waitStable waits until the group is Stable with exactly n members: every one has joined and been given
// its assignment, so what is read next is read under the final split.
func (a admin) waitStable(group string, n int) error {
	deadline := time.Now().Add(30 * time.Second)
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
