package replay

import (
	"encoding/binary"
	"fmt"
	"time"

	"github.com/twmb/franz-go/pkg/kmsg"
)

// Scenarios for the older profiles (debt K2, 2026-11-19): message formats v0 and v1, and the oldest versions of the
// offsets APIs. Recorded against Apache Kafka 3.9.1, the release --kafka-version 3 stands for, and replayed with it. Every
// record is uncompressed and every time fixed, so that the converted bytes (checksums, overflow filler and all) are
// compared byte for byte; the codecs are held to Kafka by tools/old-profiles-oracle, which compares records, not bytes.
//
//   - old-formats-produce: Produce v0, v1 and v2 carrying message sets of format v0 and v1, a v2 batch, two formats in one
//     set, a cut last message, nothing at all, a bad checksum, and a keyless message on a compacted topic; read back
//     by Fetch v4 as the v2 batches the log made of them, and by ListOffsets.
//   - old-formats-fetch: v2 batches and a transaction's markers read by Fetch v0 to v3, from several offsets and with
//     several limits: down-converted to format v0 or v1, cut at the limit, filled with overflow, the first batch converted
//     twice after a marker (Kafka's KIP-283 iterator), a marker alone answered with an overflow message.
//   - old-offsets: ListOffsets v0 (a list of offsets, newest first), OffsetCommit and OffsetFetch v0 (ZooKeeper's, which a
//     KRaft broker refuses: UNSUPPORTED_VERSION) and OffsetCommit v1 with a commit time.

func init() {
	old := func(name string, run func(*Env)) Scenario {
		return Scenario{Name: name, Run: run, Image: "apache/kafka:3.9.1", KafkaVersion: 3}
	}
	Scenarios = append(Scenarios,
		old("old-formats-produce", oldFormatsProduce),
		old("old-formats-fetch", oldFormatsFetch),
		old("old-offsets", oldOffsets),
	)
}

// oldTime is the time every record of these scenarios carries: recent enough when they were recorded that Kafka did not
// roll a segment for its age (it rolls one whose first record is a week old), fixed so that a replay writes the same bytes.
const oldTime int64 = 1795000000000

// legacySet is a message set of format magic (0 or 1), n messages with keys k0.. and values v0.., uncompressed, each
// at offset i and (v1) timestamp oldTime+i.
func legacySet(magic byte, n int) []byte {
	var out []byte
	for i := range n {
		out = append(out, legacyEntry(magic, int64(i), oldTime+int64(i), []byte(fmt.Sprintf("k%d", i)), []byte(fmt.Sprintf("v%d", i)))...)
	}
	return out
}

// legacyEntry is one message of format magic, offset and size included; a nil key is none.
func legacyEntry(magic byte, offset, ts int64, key, value []byte) []byte {
	m := []byte{magic, 0}
	if magic == 1 {
		m = binary.BigEndian.AppendUint64(m, uint64(ts))
	}
	for _, f := range [][]byte{key, value} {
		if f == nil {
			m = binary.BigEndian.AppendUint32(m, 0xffffffff)
			continue
		}
		m = binary.BigEndian.AppendUint32(m, uint32(len(f)))
		m = append(m, f...)
	}
	var out []byte
	out = binary.BigEndian.AppendUint64(out, uint64(offset))
	out = binary.BigEndian.AppendUint32(out, uint32(4+len(m)))
	out = binary.BigEndian.AppendUint32(out, crc32IEEE(m))
	return append(out, m...)
}

// v2Batch is an uncompressed v2 batch of n records (keys k0.., values v0..) at oldTime.
func v2Batch(n int) rawBatch {
	b := newBatch(n, nRecords(n, "v"))
	b.First, b.Max = oldTime, oldTime
	return b
}

// oldFetch reads partition 0 of topic from offset with a limit of max bytes at version.
func (e *Env) oldFetch(label, topic string, version int16, offset int64, max int32) {
	r := kmsg.NewPtrFetchRequest()
	r.ReplicaID, r.MaxWaitMillis, r.MinBytes, r.MaxBytes, r.SessionEpoch = -1, 0, 0, max, -1
	t := kmsg.NewFetchRequestTopic()
	t.Topic = topic
	p := kmsg.NewFetchRequestTopicPartition()
	p.FetchOffset, p.PartitionMaxBytes, p.CurrentLeaderEpoch, p.LogStartOffset = offset, max, -1, -1
	t.Partitions = append(t.Partitions, p)
	r.Topics = append(r.Topics, t)
	e.Do(label, r, version)
}

func oldFormatsProduce(e *Env) {
	e.Do("create", createTopics(false,
		newTopic("op", 1, 1),
		newTopic("opc", 1, 1, "cleanup.policy", "compact"),
	), 4)
	for v := int16(0); v <= 2; v++ {
		send := func(what string, recs []byte) { e.produceRaw(fmt.Sprintf("v%d-%s", v, what), "op", v, -1, "", recs) }
		send("m0", legacySet(0, 3))
		send("m1", legacySet(1, 3))
		send("m2", v2Batch(2).bytes())
		send("m0-and-m1", append(legacySet(0, 1), legacySet(1, 1)...))
		m1 := legacySet(1, 2)
		send("cut", m1[:len(m1)-3])
		send("empty", []byte{})
		bad := legacySet(1, 1)
		bad[len(bad)-1] ^= 0x5a
		send("bad-crc", bad)
		e.produceRaw(fmt.Sprintf("v%d-compact-no-key", v), "opc", v, -1, "", legacyEntry(1, 0, oldTime, nil, []byte("v")))
	}
	e.oldFetch("read-back", "op", 4, 0, 1<<20)
	lo := kmsg.NewPtrListOffsetsRequest()
	lo.ReplicaID = -1
	lt := kmsg.NewListOffsetsRequestTopic()
	lt.Topic = "op"
	for _, ts := range []int64{-1, -2, oldTime + 1} {
		p := kmsg.NewListOffsetsRequestTopicPartition()
		p.Timestamp, p.CurrentLeaderEpoch = ts, -1
		lt.Partitions = []kmsg.ListOffsetsRequestTopicPartition{p}
		lo.Topics = []kmsg.ListOffsetsRequestTopic{lt}
		e.Do(fmt.Sprintf("list-offsets-%d", ts), lo, 1)
	}
}

func oldFormatsFetch(e *Env) {
	e.Do("create", createTopics(false, newTopic("of", 1, 1), newTopic("oft", 1, 1)), 4)
	for i := range 3 {
		e.produceRaw(fmt.Sprintf("batch-%d", i), "of", 3, -1, "", v2Batch(3).bytes())
	}
	for v := int16(0); v <= 3; v++ {
		for _, off := range []int64{0, 1, 4, 9} {
			e.oldFetch(fmt.Sprintf("v%d-at-%d", v, off), "of", v, off, 1<<20)
		}
		for _, max := range []int32{40, 100, 150, 250} {
			e.oldFetch(fmt.Sprintf("v%d-limit-%d", v, max), "of", v, 0, max)
		}
	}
	// A committed transaction: its records, then a marker; another batch after it.
	init := kmsg.NewPtrInitProducerIDRequest()
	tx := "oft-tx"
	init.TransactionalID, init.TransactionTimeoutMillis = &tx, 60000
	find := kmsg.NewPtrFindCoordinatorRequest()
	find.CoordinatorKey, find.CoordinatorType = tx, 1
	e.DoUntil("find-coordinator", find, 1, func(r kmsg.Response) bool { return r.(*kmsg.FindCoordinatorResponse).ErrorCode == 0 })
	ir := e.DoUntil("init", init, 0, func(r kmsg.Response) bool { return r.(*kmsg.InitProducerIDResponse).ErrorCode == 0 }).(*kmsg.InitProducerIDResponse)
	add := kmsg.NewPtrAddPartitionsToTxnRequest()
	add.TransactionalID, add.ProducerID, add.ProducerEpoch = tx, ir.ProducerID, ir.ProducerEpoch
	at := kmsg.NewAddPartitionsToTxnRequestTopic()
	at.Topic, at.Partitions = "oft", []int32{0}
	add.Topics = append(add.Topics, at)
	e.DoUntil("add-partitions", add, 0, func(r kmsg.Response) bool {
		return r.(*kmsg.AddPartitionsToTxnResponse).Topics[0].Partitions[0].ErrorCode == 0
	})
	b := v2Batch(2)
	b.PID, b.Epoch, b.Seq, b.Attrs = ir.ProducerID, ir.ProducerEpoch, 0, 0x10
	e.produceRaw("txn-records", "oft", 3, -1, tx, b.bytes())
	end := kmsg.NewPtrEndTxnRequest()
	end.TransactionalID, end.ProducerID, end.ProducerEpoch, end.Commit = tx, ir.ProducerID, ir.ProducerEpoch, true
	e.DoUntil("commit", end, 0, func(r kmsg.Response) bool { return r.(*kmsg.EndTxnResponse).ErrorCode == 0 })
	e.Wait(500 * time.Millisecond)
	e.oldFetch("v0-the-marker-alone", "oft", 0, 2, 1<<20)
	e.produceRaw("after-the-marker", "oft", 3, -1, "", v2Batch(1).bytes())
	for v := int16(0); v <= 3; v++ {
		for _, off := range []int64{0, 2} {
			e.oldFetch(fmt.Sprintf("txn-v%d-at-%d", v, off), "oft", v, off, 1<<20)
		}
	}
}

func oldOffsets(e *Env) {
	e.Do("create", createTopics(false, newTopic("oo", 2, 1)), 4)
	for i := range 2 {
		e.produceRaw(fmt.Sprintf("batch-%d", i), "oo", 3, -1, "", v2Batch(2).bytes())
	}
	for _, ts := range []int64{-1, -2, 0} {
		for _, n := range []int32{1, 2, 10} {
			lo := kmsg.NewPtrListOffsetsRequest()
			lo.ReplicaID = -1
			lt := kmsg.NewListOffsetsRequestTopic()
			lt.Topic = "oo"
			p := kmsg.NewListOffsetsRequestTopicPartition()
			p.Timestamp, p.MaxNumOffsets = ts, n
			lt.Partitions = append(lt.Partitions, p)
			lo.Topics = append(lo.Topics, lt)
			e.Do(fmt.Sprintf("list-offsets-v0-%d-%d", ts, n), lo, 0)
		}
	}
	find := kmsg.NewPtrFindCoordinatorRequest()
	find.CoordinatorKey = "og"
	e.DoUntil("find-coordinator", find, 1, func(r kmsg.Response) bool { return r.(*kmsg.FindCoordinatorResponse).ErrorCode == 0 })
	ready := kmsg.NewPtrOffsetFetchRequest()
	ready.Group = "og"
	e.DoUntil("group-ready", ready, 2, func(r kmsg.Response) bool { return r.(*kmsg.OffsetFetchResponse).ErrorCode == 0 })
	for v := int16(0); v <= 1; v++ {
		c := kmsg.NewPtrOffsetCommitRequest()
		c.Group, c.Generation, c.MemberID = "og", -1, ""
		t := kmsg.NewOffsetCommitRequestTopic()
		t.Topic = "oo"
		p := kmsg.NewOffsetCommitRequestTopicPartition()
		p.Partition, p.Offset, p.Timestamp = int32(v), 10+int64(v), oldTime
		meta := fmt.Sprintf("m%d", v)
		p.Metadata = &meta
		t.Partitions = append(t.Partitions, p)
		unknown := kmsg.NewOffsetCommitRequestTopic()
		unknown.Topic = "nothing"
		unknown.Partitions = []kmsg.OffsetCommitRequestTopicPartition{p}
		c.Topics = []kmsg.OffsetCommitRequestTopic{t}
		e.Do(fmt.Sprintf("commit-v%d", v), c, v)
		c.Topics = []kmsg.OffsetCommitRequestTopic{unknown}
		e.Do(fmt.Sprintf("commit-v%d-unknown-topic", v), c, v)
		for f := int16(0); f <= 2; f++ {
			r := kmsg.NewPtrOffsetFetchRequest()
			r.Group = "og"
			ft := kmsg.NewOffsetFetchRequestTopic()
			ft.Topic, ft.Partitions = "oo", []int32{0, 1}
			r.Topics = append(r.Topics, ft)
			e.Do(fmt.Sprintf("after-v%d-fetch-v%d", v, f), r, f)
		}
	}
}
