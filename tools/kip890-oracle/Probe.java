// KIP-890 oracle probe (docs/port/behaviours/transactions.md, TX-16 to TX-25): raw requests at chosen versions, put to a
// broker over one plain connection, the answers printed one line each with producer ids normalised (P0, P1, ... per
// scenario) and the run's names replaced by R, so that two brokers' outputs can be diffed.
//
//   cd interop && java -cp "$(bun java/fetch.ts 4)" ../tools/kip890-oracle/Probe.java -broker host:port [-only name] [-skip-exhaust]
//
// kafka-4.4.0.txt is what a real Apache Kafka 4.4.0 answered (KRaft, one node in combined mode, formatted with the defaults
// of 4.4.0, so transaction.version 2; transaction.state.log.replication.factor=1, transaction.state.log.min.isr=1,
// offsets.topic.num.partitions=1, transaction.abort.timed.out.transaction.cleanup.interval.ms=1000). Against this broker
// the output differs from it only where TX-25 and TX-16 say: the features listed, the API ranges (Produce from 3 and to
// 12, TxnOffsetCommit to 5), the offsets partition a group maps to (50 partitions here), and the CONCURRENT_TRANSACTIONS
// Kafka answers an InitProducerId first while it aborts (owninit). `exhaust` uses up 32,764 epochs (a minute or more).
import org.apache.kafka.common.message.*;
import org.apache.kafka.common.protocol.ApiKeys;
import org.apache.kafka.common.protocol.ApiMessage;
import org.apache.kafka.common.compress.Compression;
import org.apache.kafka.common.record.internal.*;
import org.apache.kafka.common.requests.*;

import java.io.*;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.util.*;

public class Probe {
    static Socket sock;
    static DataInputStream in;
    static DataOutputStream out;
    static int corr = 0;
    static String run = Long.toString(System.currentTimeMillis() % 1_000_000, 36);
    static Map<Long, String> pids = new HashMap<>();

    static String pid(long p) {
        if (p < 0) return Long.toString(p);
        return pids.computeIfAbsent(p, k -> "P" + pids.size());
    }

    static ApiMessage ask(ApiKeys key, short v, ApiMessage data) throws IOException {
        RequestHeader h = new RequestHeader(key, v, "probe", ++corr);
        ByteBuffer b = RequestUtils.serialize(h.data(), h.headerVersion(), data, v);
        out.writeInt(b.remaining());
        out.write(b.array(), b.arrayOffset() + b.position(), b.remaining());
        out.flush();
        int n = in.readInt();
        byte[] r = new byte[n];
        in.readFully(r);
        return AbstractResponse.parseResponse(ByteBuffer.wrap(r), h).data();
    }

    static void say(String label, String what) {
        System.out.println(label + ": " + what);
    }

    // ---- requests ----
    static void topic(String name, int parts) throws IOException {
        var d = new CreateTopicsRequestData().setTimeoutMs(10000);
        d.topics().add(new CreateTopicsRequestData.CreatableTopic().setName(name).setNumPartitions(parts).setReplicationFactor((short) 1));
        var r = (CreateTopicsResponseData) ask(ApiKeys.CREATE_TOPICS, (short) 7, d);
        short e = r.topics().iterator().next().errorCode();
        if (e != 0) throw new IllegalStateException("create " + name + ": " + e);
        // Kafka's partitions are hosted a moment after the topic is made: wait until each answers ListOffsets.
        for (int part = 0; part < parts; part++) {
            for (int i = 0; ; i++) {
                var lo = new ListOffsetsRequestData().setReplicaId(-1).setIsolationLevel((byte) 0);
                var lt = new ListOffsetsRequestData.ListOffsetsTopic().setName(name);
                lt.partitions().add(new ListOffsetsRequestData.ListOffsetsPartition().setPartitionIndex(part).setTimestamp(-1).setCurrentLeaderEpoch(-1));
                lo.topics().add(lt);
                var lr = (ListOffsetsResponseData) ask(ApiKeys.LIST_OFFSETS, (short) 7, lo);
                if (lr.topics().get(0).partitions().get(0).errorCode() == 0) break;
                if (i > 100) throw new IllegalStateException("partition " + name + "-" + part + " never hosted");
                sleep(100);
            }
        }
    }

    static void groupCoordinator(String g) throws IOException {
        for (int i = 0; i < 100; i++) {
            var d = new FindCoordinatorRequestData().setKeyType((byte) 0).setCoordinatorKeys(List.of(g));
            var r = (FindCoordinatorResponseData) ask(ApiKeys.FIND_COORDINATOR, (short) 4, d);
            if (r.coordinators().get(0).errorCode() == 0) return;
            sleep(200);
        }
        throw new IllegalStateException("no coordinator for " + g);
    }

    /** Waits for an ended transaction's markers (Kafka writes them after it answers). */
    static void settled(String tx) throws IOException {
        for (int i = 0; i < 100; i++) {
            String d = describe(tx);
            if (!d.startsWith("Prepare")) return;
            sleep(50);
        }
    }

    record Pe(long pid, short epoch) {
        public String toString() { return Probe.pid(pid) + "/" + epoch; }
    }

    static Object init(String tx, int timeout, short v, long pid, short epoch) throws IOException {
        var d = new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(timeout).setProducerId(pid).setProducerEpoch(epoch);
        var r = (InitProducerIdResponseData) ask(ApiKeys.INIT_PRODUCER_ID, v, d);
        if (r.errorCode() != 0) return "error " + r.errorCode();
        return new Pe(r.producerId(), r.producerEpoch());
    }

    static void coordinator(String tx) throws IOException {
        for (int i = 0; i < 100; i++) {
            var d = new FindCoordinatorRequestData().setKeyType((byte) 1).setCoordinatorKeys(List.of(tx));
            var r = (FindCoordinatorResponseData) ask(ApiKeys.FIND_COORDINATOR, (short) 4, d);
            if (r.coordinators().get(0).errorCode() == 0) return;
            sleep(200);
        }
        throw new IllegalStateException("no coordinator for " + tx);
    }

    static Pe init(String tx, int timeout) throws IOException {
        coordinator(tx);
        Object o = init(tx, timeout, (short) 5, -1, (short) -1);
        if (o instanceof Pe p) return p;
        // Kafka answers CONCURRENT_TRANSACTIONS while it aborts what a previous owner left open: ask again.
        for (int i = 0; i < 50 && !(o instanceof Pe); i++) {
            sleep(100);
            o = init(tx, timeout, (short) 5, -1, (short) -1);
        }
        if (o instanceof Pe p) return p;
        throw new IllegalStateException("init " + tx + ": " + o);
    }

    static String produce(short v, String tx, String topic, int part, Pe p, int seq, boolean transactional) throws IOException {
        MemoryRecords recs = transactional
            ? MemoryRecords.withTransactionalRecords(Compression.NONE, p.pid, p.epoch, seq, new SimpleRecord(("v" + seq).getBytes()))
            : MemoryRecords.withIdempotentRecords(Compression.NONE, p.pid, p.epoch, seq, new SimpleRecord(("v" + seq).getBytes()));
        var d = new ProduceRequestData().setAcks((short) -1).setTimeoutMs(10000).setTransactionalId(tx);
        var td = new ProduceRequestData.TopicProduceData().setName(topic);
        td.partitionData().add(new ProduceRequestData.PartitionProduceData().setIndex(part).setRecords(recs));
        d.topicData().add(td);
        var r = (ProduceResponseData) ask(ApiKeys.PRODUCE, v, d);
        var pr = r.responses().iterator().next().partitionResponses().get(0);
        return pr.errorCode() == 0 ? "ok@" + pr.baseOffset() : "error " + pr.errorCode();
    }

    static String end(short v, String tx, Pe p, boolean commit) throws IOException {
        var d = new EndTxnRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setCommitted(commit);
        for (int i = 0; ; i++) {
            var r = (EndTxnResponseData) ask(ApiKeys.END_TXN, v, d);
            // Kafka answers CONCURRENT_TRANSACTIONS while the markers of an earlier end are written: the client asks again.
            if (r.errorCode() == 51 && i < 50) { sleep(100); continue; }
            String s = r.errorCode() == 0 ? "ok" : "error " + r.errorCode();
            if (v >= 5) s += " " + (r.producerId() < 0 ? r.producerId() + "/" + r.producerEpoch() : new Pe(r.producerId(), r.producerEpoch()));
            if (r.errorCode() == 0) settled(tx);
            return s;
        }
    }

    static boolean noSettle = false;

    static Pe endOk(String tx, Pe p, boolean commit) throws IOException {
        var d = new EndTxnRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setCommitted(commit);
        for (int i = 0; i < 50; i++) {
            var r = (EndTxnResponseData) ask(ApiKeys.END_TXN, (short) 5, d);
            if (r.errorCode() == 51) { sleep(100); continue; }
            if (r.errorCode() != 0) throw new IllegalStateException("end " + tx + ": " + r.errorCode());
            if (!noSettle) settled(tx);
            return new Pe(r.producerId(), r.producerEpoch());
        }
        throw new IllegalStateException("end " + tx + ": still concurrent");
    }

    static String addParts(short v, String tx, Pe p, String topic, int... parts) throws IOException {
        var d = new AddPartitionsToTxnRequestData();
        var topics = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopicCollection();
        var t = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopic().setName(topic);
        for (int x : parts) t.partitions().add(x);
        topics.add(t);
        d.setV3AndBelowTransactionalId(tx).setV3AndBelowProducerId(p.pid).setV3AndBelowProducerEpoch(p.epoch).setV3AndBelowTopics(topics);
        var r = (AddPartitionsToTxnResponseData) ask(ApiKeys.ADD_PARTITIONS_TO_TXN, v, d);
        StringBuilder sb = new StringBuilder();
        r.resultsByTopicV3AndBelow().forEach(tr -> tr.resultsByPartition().forEach(pr -> sb.append(tr.name().replace(run, "R")).append("-").append(pr.partitionIndex()).append("=").append(pr.partitionErrorCode()).append(" ")));
        return sb.toString().trim();
    }

    record TxnAdd(String tx, Pe p, boolean verify, String topic, int[] parts) {}

    static String addPartsBatch(short v, TxnAdd... txns) throws IOException {
        var d = new AddPartitionsToTxnRequestData();
        var coll = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTransactionCollection();
        for (TxnAdd a : txns) {
            var topics = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopicCollection();
            var t = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopic().setName(a.topic);
            for (int x : a.parts) t.partitions().add(x);
            topics.add(t);
            coll.add(new AddPartitionsToTxnRequestData.AddPartitionsToTxnTransaction().setTransactionalId(a.tx)
                .setProducerId(a.p.pid).setProducerEpoch(a.p.epoch).setVerifyOnly(a.verify).setTopics(topics));
        }
        d.setTransactions(coll);
        var r = (AddPartitionsToTxnResponseData) ask(ApiKeys.ADD_PARTITIONS_TO_TXN, v, d);
        StringBuilder sb = new StringBuilder("top=" + r.errorCode());
        List<String> each = new ArrayList<>();
        r.resultsByTransaction().forEach(tr -> {
            List<String> ps = new ArrayList<>();
            tr.topicResults().forEach(t -> t.resultsByPartition().forEach(pr -> ps.add(t.name().replace(run, "R") + "-" + pr.partitionIndex() + "=" + pr.partitionErrorCode())));
            Collections.sort(ps);
            each.add(" [" + tr.transactionalId().replace(run, "R") + ": " + String.join(" ", ps) + "]");
        });
        Collections.sort(each);
        each.forEach(sb::append);
        return sb.toString();
    }

    static String addOffsets(short v, String tx, Pe p, String group) throws IOException {
        var d = new AddOffsetsToTxnRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setGroupId(group);
        var r = (AddOffsetsToTxnResponseData) ask(ApiKeys.ADD_OFFSETS_TO_TXN, v, d);
        return r.errorCode() == 0 ? "ok" : "error " + r.errorCode();
    }

    static String txnOffset(short v, String tx, Pe p, String group, String topic, int part, long offset) throws IOException {
        var d = new TxnOffsetCommitRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setGroupId(group)
            .setMemberId("").setGenerationIdOrMemberEpoch(-1);
        var t = new TxnOffsetCommitRequestData.TxnOffsetCommitRequestTopic().setName(topic);
        t.partitions().add(new TxnOffsetCommitRequestData.TxnOffsetCommitRequestPartition().setPartitionIndex(part).setCommittedOffset(offset).setCommittedLeaderEpoch(-1));
        d.topics().add(t);
        for (int i = 0; ; i++) {
            var r = (TxnOffsetCommitResponseData) ask(ApiKeys.TXN_OFFSET_COMMIT, v, d);
            short e = r.topics().get(0).partitions().get(0).errorCode();
            if ((e == 51) && i < 50) { sleep(100); continue; }
            return e == 0 ? "ok" : "error " + e;
        }
    }

    static String committed(String group, String topic, int part) throws IOException {
        var d = new OffsetFetchRequestData().setGroupId(group).setRequireStable(true);
        d.topics().add(new OffsetFetchRequestData.OffsetFetchRequestTopic().setName(topic).setPartitionIndexes(List.of(part)));
        var r = (OffsetFetchResponseData) ask(ApiKeys.OFFSET_FETCH, (short) 7, d);
        var pr = r.topics().get(0).partitions().get(0);
        return pr.errorCode() == 0 ? "offset " + pr.committedOffset() : "error " + pr.errorCode() + " offset " + pr.committedOffset();
    }

    static String describe(String tx) throws IOException {
        var d = new DescribeTransactionsRequestData().setTransactionalIds(List.of(tx));
        var r = (DescribeTransactionsResponseData) ask(ApiKeys.DESCRIBE_TRANSACTIONS, (short) 0, d);
        var s = r.transactionStates().get(0);
        if (s.errorCode() != 0) return "error " + s.errorCode();
        List<String> parts = new ArrayList<>();
        s.topics().forEach(t -> t.partitions().forEach(x -> parts.add(t.topic().replace(run, "R").replaceAll("__consumer_offsets", "OFFSETS") + "-" + x)));
        Collections.sort(parts);
        return s.transactionState() + " " + new Pe(s.producerId(), s.producerEpoch()) + " " + parts;
    }

    static String producers(String topic, int part) throws IOException {
        var d = new DescribeProducersRequestData();
        d.topics().add(new DescribeProducersRequestData.TopicRequest().setName(topic).setPartitionIndexes(List.of(part)));
        var r = (DescribeProducersResponseData) ask(ApiKeys.DESCRIBE_PRODUCERS, (short) 0, d);
        var pr = r.topics().get(0).partitions().get(0);
        if (pr.errorCode() != 0) return "error " + pr.errorCode();
        List<String> each = new ArrayList<>();
        pr.activeProducers().forEach(a -> each.add(pid(a.producerId()) + " epoch " + a.producerEpoch() + " seq " + a.lastSequence() + " txnStart " + a.currentTxnStartOffset()));
        Collections.sort(each);
        return each.toString();
    }

    static String fetchCommitted(String topic, int part) throws IOException {
        var d = new FetchRequestData().setMaxWaitMs(0).setMinBytes(0).setMaxBytes(1 << 20).setIsolationLevel((byte) 1).setReplicaId(-1);
        var ft = new FetchRequestData.FetchTopic().setTopic(topic);
        ft.partitions().add(new FetchRequestData.FetchPartition().setPartition(part).setFetchOffset(0).setPartitionMaxBytes(1 << 20).setLogStartOffset(-1));
        d.topics().add(ft);
        var r = (FetchResponseData) ask(ApiKeys.FETCH, (short) 12, d);
        var pd = r.responses().get(0).partitions().get(0);
        int aborted = pd.abortedTransactions() == null ? 0 : pd.abortedTransactions().size();
        return "hw " + pd.highWatermark() + " lso " + pd.lastStableOffset() + " aborted " + aborted;
    }

    static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException e) { throw new RuntimeException(e); }
    }

    // ---- scenarios ----
    static String t(String s) { return "kip890-" + run + "-" + s; }

    static void features() throws IOException {
        var d = new ApiVersionsRequestData().setClientSoftwareName("probe").setClientSoftwareVersion("1");
        var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, (short) 4, d);
        r.supportedFeatures().forEach(f -> say("features", "supported " + f.name() + " " + f.minVersion() + ".." + f.maxVersion()));
        r.finalizedFeatures().forEach(f -> say("features", "finalized " + f.name() + " " + f.minVersionLevel() + ".." + f.maxVersionLevel()));
        say("features", "epoch>=0 " + (r.finalizedFeaturesEpoch() >= 0) + " zkMigrationReady " + r.zkMigrationReady());
        r.apiKeys().forEach(k -> {
            if (List.of(0, 22, 24, 25, 26, 28).contains((int) k.apiKey())) say("features", "api " + k.apiKey() + " " + k.minVersion() + ".." + k.maxVersion());
        });
        var r3 = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, (short) 3, d);
        say("features", "v3 finalized " + r3.finalizedFeatures().size() + " supported " + r3.supportedFeatures().size());
    }

    /** Produce v12 adds the partition; EndTxn v5 bumps the epoch and says so; the next transaction runs at the bumped epoch. */
    static void basic() throws IOException {
        String L = "basic", tp = t("basic"), tx = tp + "-tx";
        topic(tp, 2);
        Pe p = init(tx, 60000);
        say(L, "init " + p);
        say(L, "produce v12 p0 " + produce((short) 12, tx, tp, 0, p, 0, true));
        say(L, "describe " + describe(tx));
        say(L, "producers p0 " + producers(tp, 0));
        say(L, "lso " + fetchCommitted(tp, 0));
        say(L, "produce v12 p0 seq1 " + produce((short) 12, tx, tp, 0, p, 1, true));
        say(L, "produce v12 p1 " + produce((short) 12, tx, tp, 1, p, 0, true));
        String e = end((short) 5, tx, p, true);
        say(L, "end v5 commit " + e);
        say(L, "describe " + describe(tx));
        say(L, "producers p0 " + producers(tp, 0));
        say(L, "lso " + fetchCommitted(tp, 0));
        Pe q = new Pe(p.pid, (short) (p.epoch + 1));
        say(L, "produce v12 old epoch p0 " + produce((short) 12, tx, tp, 0, p, 2, true));
        say(L, "produce v12 new epoch seq 0 p0 " + produce((short) 12, tx, tp, 0, q, 0, true));
        say(L, "end v5 abort " + end((short) 5, tx, q, false));
        say(L, "lso " + fetchCommitted(tp, 0));
        say(L, "producers p0 " + producers(tp, 0));
        say(L, "producers p1 " + producers(tp, 1));
    }

    /** The table of TransactionCoordinator.endTransaction for TV2: Empty, CompleteCommit, CompleteAbort with the current and the retry epoch. */
    static void table() throws IOException {
        String L = "table", tp = t("table"), tx = tp + "-tx";
        topic(tp, 1);
        Pe p = init(tx, 60000);
        Pe before = new Pe(p.pid, (short) (p.epoch - 1));
        say(L, "Empty commit current " + end((short) 5, tx, p, true));
        say(L, "Empty commit retry " + end((short) 5, tx, before, true));
        say(L, "Empty abort retry " + end((short) 5, tx, before, false));
        say(L, "Empty abort current " + end((short) 5, tx, p, false));
        say(L, "describe " + describe(tx));
        Pe p1 = new Pe(p.pid, (short) (p.epoch + 1));
        // CompleteAbort at p1; the retry epoch is p.
        say(L, "CompleteAbort commit retry " + end((short) 5, tx, p, true));
        say(L, "CompleteAbort abort retry " + end((short) 5, tx, p, false));
        say(L, "CompleteAbort commit current " + end((short) 5, tx, p1, true));
        say(L, "CompleteAbort higher " + end((short) 5, tx, new Pe(p.pid, (short) (p.epoch + 5)), false));
        say(L, "CompleteAbort other pid " + end((short) 5, tx, new Pe(p.pid + 1000, p1.epoch), false));
        say(L, "CompleteAbort two back " + end((short) 5, tx, new Pe(p.pid, (short) (p.epoch - 1)), false));
        say(L, "produce v12 " + produce((short) 12, tx, tp, 0, p1, 0, true));
        say(L, "Ongoing abort retry " + end((short) 5, tx, p, false));
        say(L, "Ongoing commit current " + end((short) 5, tx, p1, true));
        Pe p2 = new Pe(p.pid, (short) (p.epoch + 2));
        say(L, "CompleteCommit commit retry " + end((short) 5, tx, p1, true));
        say(L, "CompleteCommit abort retry " + end((short) 5, tx, p1, false));
        say(L, "CompleteCommit commit current " + end((short) 5, tx, p2, true));
        say(L, "CompleteCommit abort current " + end((short) 5, tx, p2, false));
        say(L, "describe " + describe(tx));
        // The classic EndTxn on a state TV2 left.
        Pe p3 = new Pe(p.pid, (short) (p.epoch + 3));
        say(L, "v4 CompleteAbort abort current " + end((short) 4, tx, p3, false));
        say(L, "v4 CompleteAbort abort retry " + end((short) 4, tx, p2, false));
        say(L, "produce v12 " + produce((short) 12, tx, tp, 0, p3, 0, true));
        say(L, "v4 Ongoing commit " + end((short) 4, tx, p3, true));
        say(L, "describe " + describe(tx));
        say(L, "producers " + producers(tp, 0));
        say(L, "v5 CompleteCommit after v4 commit current " + end((short) 5, tx, p3, true));
        say(L, "v5 CompleteCommit after v4 abort current " + end((short) 5, tx, p3, false));
    }

    /** Produce to a partition that was not added, at each version of the classic path; v12 adds it. */
    static void unadded() throws IOException {
        String L = "unadded", tp = t("unadded"), tx = tp + "-tx";
        topic(tp, 3);
        Pe p = init(tx, 60000);
        for (short v : new short[]{3, 9, 10, 11}) say(L, "produce v" + v + " not added " + produce(v, tx, tp, 0, p, 0, true));
        say(L, "describe " + describe(tx));
        say(L, "add v3 " + addParts((short) 3, tx, p, tp, 1));
        say(L, "produce v9 p0 still not added " + produce((short) 9, tx, tp, 0, p, 0, true));
        say(L, "produce v11 p0 still not added " + produce((short) 11, tx, tp, 0, p, 0, true));
        say(L, "produce v9 p1 added " + produce((short) 9, tx, tp, 1, p, 0, true));
        say(L, "produce v12 p2 " + produce((short) 12, tx, tp, 2, p, 0, true));
        say(L, "describe " + describe(tx));
        say(L, "end v3 commit " + end((short) 3, tx, p, true));
        say(L, "describe " + describe(tx));
        say(L, "producers p1 " + producers(tp, 1));
        say(L, "produce v11 after end " + produce((short) 11, tx, tp, 1, p, 1, true));
        say(L, "produce v12 after classic end " + produce((short) 12, tx, tp, 1, p, 1, true));
        say(L, "end v5 commit " + end((short) 5, tx, p, true));
    }

    /** Produce v12 refusals: a first batch not at 0, a stale epoch, a stranger's producer id, an id nobody owns. */
    static void refusals() throws IOException {
        String L = "refusals", tp = t("refusals"), tx = tp + "-tx";
        topic(tp, 3);
        Pe p = endOk(tx, init(tx, 60000), false);
        say(L, "produce v12 first at seq 5 " + produce((short) 12, tx, tp, 0, p, 5, true));
        say(L, "describe " + describe(tx));
        say(L, "produce v12 seq 0 after " + produce((short) 12, tx, tp, 0, p, 0, true));
        say(L, "produce v12 old epoch " + produce((short) 12, tx, tp, 1, new Pe(p.pid, (short) (p.epoch - 1)), 0, true));
        say(L, "produce v12 higher epoch " + produce((short) 12, tx, tp, 1, new Pe(p.pid, (short) (p.epoch + 1)), 0, true));
        say(L, "produce v12 other pid " + produce((short) 12, tx, tp, 1, new Pe(p.pid + 1000, p.epoch), 0, true));
        say(L, "produce v12 unknown tx " + produce((short) 12, tx + "-nobody", tp, 1, p, 0, true));
        say(L, "produce v11 unknown tx " + produce((short) 11, tx + "-nobody", tp, 1, p, 0, true));
        say(L, "describe " + describe(tx));
        say(L, "produce v12 idempotent (not transactional) " + produce((short) 12, null, tp, 2, p, 0, false));
        say(L, "end v5 abort " + end((short) 5, tx, p, false));
        say(L, "lso p0 " + fetchCommitted(tp, 0));
        say(L, "producers p0 " + producers(tp, 0));
        say(L, "producers p1 " + producers(tp, 1));
    }

    /** TxnOffsetCommit without AddOffsetsToTxn at v3, v4, v5; then committed with the transaction. */
    static void offsets() throws IOException {
        String L = "offsets", tp = t("offsets"), tx = tp + "-tx", g = tp + "-g";
        topic(tp, 1);
        groupCoordinator(g);
        groupCoordinator(g + "2");
        Pe p = init(tx, 60000);
        say(L, "txnoffset v3 not added " + txnOffset((short) 3, tx, p, g, tp, 0, 5));
        say(L, "txnoffset v4 not added " + txnOffset((short) 4, tx, p, g, tp, 0, 5));
        say(L, "describe " + describe(tx));
        say(L, "txnoffset v5 implicit " + txnOffset((short) 5, tx, p, g, tp, 0, 5));
        say(L, "describe " + describe(tx));
        say(L, "committed " + committed(g, tp, 0));
        say(L, "txnoffset v3 after v5 added " + txnOffset((short) 3, tx, p, g, tp, 0, 6));
        say(L, "end v5 commit " + end((short) 5, tx, p, true));
        say(L, "committed " + committed(g, tp, 0));
        Pe q = new Pe(p.pid, (short) (p.epoch + 1));
        say(L, "txnoffset v5 old epoch " + txnOffset((short) 5, tx, p, g, tp, 0, 9));
        say(L, "addoffsets v4 " + addOffsets((short) 4, tx, q, g));
        say(L, "txnoffset v4 after add " + txnOffset((short) 4, tx, q, g, tp, 0, 8));
        say(L, "end v5 abort " + end((short) 5, tx, q, false));
        say(L, "committed " + committed(g, tp, 0));
        Pe r = new Pe(p.pid, (short) (p.epoch + 2));
        say(L, "txnoffset v5 other group " + txnOffset((short) 5, tx, r, g + "2", tp, 0, 3));
        say(L, "end v5 commit " + end((short) 5, tx, r, true));
        say(L, "committed other " + committed(g + "2", tp, 0));
        say(L, "addoffsets v4 old epoch " + addOffsets((short) 4, tx, r, g));
        say(L, "addoffsets v3 old epoch " + addOffsets((short) 3, tx, r, g));
        say(L, "addoffsets v1 old epoch " + addOffsets((short) 1, tx, r, g));
    }

    /** AddPartitionsToTxn v4 and v5: several transactions, verify-only and adding. */
    static void batched() throws IOException {
        String L = "batched", tp = t("batched"), a = tp + "-a", b = tp + "-b";
        topic(tp, 3);
        Pe pa = init(a, 60000), pb = init(b, 60000);
        say(L, "add v3 a p0 " + addParts((short) 3, a, pa, tp, 0));
        for (short v : new short[]{4, 5}) {
            say(L, "v" + v + " verify a p0,p1 + b p0 " + addPartsBatch(v, new TxnAdd(a, pa, true, tp, new int[]{0, 1}), new TxnAdd(b, pb, true, tp, new int[]{0})));
        }
        say(L, "v5 verify unknown tx " + addPartsBatch((short) 5, new TxnAdd(a + "-nobody", pa, true, tp, new int[]{0})));
        say(L, "v5 verify stale epoch " + addPartsBatch((short) 5, new TxnAdd(a, new Pe(pa.pid, (short) (pa.epoch - 1)), true, tp, new int[]{0})));
        say(L, "v5 verify other pid " + addPartsBatch((short) 5, new TxnAdd(a, new Pe(pa.pid + 1000, pa.epoch), true, tp, new int[]{0})));
        say(L, "v5 verify unknown partition " + addPartsBatch((short) 5, new TxnAdd(a, pa, true, tp, new int[]{0, 9})));
        say(L, "v5 verify unknown topic " + addPartsBatch((short) 5, new TxnAdd(a, pa, true, tp + "-none", new int[]{0})));
        say(L, "v5 add b p1,p2 " + addPartsBatch((short) 5, new TxnAdd(b, pb, false, tp, new int[]{1, 2})));
        say(L, "describe b " + describe(b));
        say(L, "v5 verify b p1 " + addPartsBatch((short) 5, new TxnAdd(b, pb, true, tp, new int[]{1})));
        say(L, "produce v11 b p1 " + produce((short) 11, b, tp, 1, pb, 0, true));
        say(L, "v4 add stale " + addPartsBatch((short) 4, new TxnAdd(b, new Pe(pb.pid, (short) (pb.epoch - 1)), false, tp, new int[]{0})));
        say(L, "v5 add empty id " + addPartsBatch((short) 5, new TxnAdd("", pb, false, tp, new int[]{0})));
        say(L, "v5 verify empty id " + addPartsBatch((short) 5, new TxnAdd("", pb, true, tp, new int[]{0})));
        say(L, "end v5 b commit " + end((short) 5, b, pb, true));
        say(L, "v5 verify b after end " + addPartsBatch((short) 5, new TxnAdd(b, new Pe(pb.pid, (short) (pb.epoch + 1)), true, tp, new int[]{1})));
        say(L, "end v3 a abort " + end((short) 3, a, pa, false));
    }

    /** A transaction that outlives its timeout, with a TV2 producer and a classic one. */
    static void timeout() throws IOException {
        String L = "timeout", tp = t("timeout"), tx2 = tp + "-tx2", tx1 = tp + "-tx1";
        topic(tp, 1);
        Pe p2 = init(tx2, 1000), p1 = init(tx1, 1000);
        say(L, "tv2 produce v12 " + produce((short) 12, tx2, tp, 0, p2, 0, true));
        say(L, "tv1 add v3 " + addParts((short) 3, tx1, p1, tp, 0));
        say(L, "tv1 produce v9 " + produce((short) 9, tx1, tp, 0, p1, 0, true));
        sleep(4000);
        say(L, "describe tv2 " + describe(tx2));
        say(L, "describe tv1 " + describe(tx1));
        say(L, "producers " + producers(tp, 0));
        say(L, "tv2 produce v12 after timeout " + produce((short) 12, tx2, tp, 0, p2, 1, true));
        say(L, "tv2 end v5 abort old epoch " + end((short) 5, tx2, p2, false));
        say(L, "tv2 end v5 commit old epoch " + end((short) 5, tx2, p2, true));
        say(L, "tv1 end v3 abort old epoch " + end((short) 3, tx1, p1, false));
        say(L, "tv1 init v4 with old epoch " + init(tx1, 1000, (short) 4, p1.pid, p1.epoch));
        say(L, "describe tv1 " + describe(tx1));
        say(L, "tv2 init v4 with old epoch " + init(tx2, 1000, (short) 4, p2.pid, p2.epoch));
        say(L, "describe tv2 " + describe(tx2));
    }

    /** A new owner takes the id over while a TV2 transaction is open: the old producer is fenced for good. */
    static void zombie() throws IOException {
        String L = "zombie", tp = t("zombie"), tx = tp + "-tx";
        topic(tp, 1);
        Pe a = init(tx, 60000);
        say(L, "a produce v12 " + produce((short) 12, tx, tp, 0, a, 0, true));
        Pe b = init(tx, 60000);
        say(L, "b init " + b + " (a was " + a + ")");
        say(L, "describe " + describe(tx));
        say(L, "producers " + producers(tp, 0));
        say(L, "a produce v12 " + produce((short) 12, tx, tp, 0, a, 1, true));
        say(L, "a end v5 abort " + end((short) 5, tx, a, false));
        say(L, "a end v5 commit " + end((short) 5, tx, a, true));
        say(L, "a init v4 own epoch " + init(tx, 60000, (short) 4, a.pid, a.epoch));
        say(L, "b produce v12 " + produce((short) 12, tx, tp, 0, b, 0, true));
        say(L, "b end v5 commit " + end((short) 5, tx, b, true));
        say(L, "lso " + fetchCommitted(tp, 0));
    }

    /** The epochs of a producer id run out under TV2: EndTxn v5 hands out a new producer id at epoch 0. */
    static void exhaust() throws IOException {
        String L = "exhaust", tp = t("exhaust"), tx = tp + "-tx";
        topic(tp, 1);
        Pe p = init(tx, 60000);
        long t0 = System.currentTimeMillis();
        noSettle = true;
        while (p.epoch < Short.MAX_VALUE - 3) p = endOk(tx, p, false);
        noSettle = false;
        say(L, "aborted empty up to " + p.epoch);
        System.err.println("exhaust: " + (System.currentTimeMillis() - t0) / 1000 + "s to use up the epochs");
        say(L, "produce v12 at " + p.epoch + " " + produce((short) 12, tx, tp, 0, p, 0, true));
        Pe q = endOk(tx, p, true);
        say(L, "commit -> " + q);
        say(L, "produce v12 at " + q.epoch + " " + produce((short) 12, tx, tp, 0, q, 0, true));
        say(L, "commit -> " + (q = endOk(tx, q, true)));
        say(L, "describe " + describe(tx));
        say(L, "producers " + producers(tp, 0));
        say(L, "produce v12 at " + q.epoch + " " + produce((short) 12, tx, tp, 0, q, 0, true));
        Pe last = q;
        String e = end((short) 5, tx, q, true);
        say(L, "commit at " + q.epoch + " -> " + e);
        say(L, "describe " + describe(tx));
        say(L, "producers " + producers(tp, 0));
        say(L, "retry commit at " + last.epoch + " -> " + end((short) 5, tx, last, true));
        say(L, "retry abort at " + last.epoch + " -> " + end((short) 5, tx, last, false));
        say(L, "old pid at 32765 -> " + end((short) 5, tx, new Pe(last.pid, (short) 32765), true));
        say(L, "init v4 old pid at " + last.epoch + " -> " + init(tx, 60000, (short) 4, last.pid, last.epoch));
        say(L, "describe " + describe(tx));
        say(L, "lso " + fetchCommitted(tp, 0));
    }

    /** The owner asks InitProducerId with its own producer id and epoch while its transaction is open (KIP-360). */
    static void owninit() throws IOException {
        String L = "owninit", tp = t("owninit"), tx2 = tp + "-tx2", tx1 = tp + "-tx1";
        topic(tp, 1);
        Pe p2 = init(tx2, 60000), p1 = init(tx1, 60000);
        say(L, "tv2 produce v12 " + produce((short) 12, tx2, tp, 0, p2, 0, true));
        say(L, "tv1 add v3 " + addParts((short) 3, tx1, p1, tp, 0));
        say(L, "tv1 produce v9 " + produce((short) 9, tx1, tp, 0, p1, 0, true));
        for (String[] c : new String[][]{{tx2, "tv2"}, {tx1, "tv1"}}) {
            Pe p = c[0].equals(tx2) ? p2 : p1;
            Object o = init(c[0], 60000, (short) 4, p.pid, p.epoch);
            int tries = 0;
            while ("error 51".equals(o) && tries++ < 50) { sleep(100); o = init(c[0], 60000, (short) 4, p.pid, p.epoch); }
            say(L, c[1] + " init v4 own epoch while open -> " + o + (tries > 0 ? " (after CONCURRENT_TRANSACTIONS)" : ""));
            say(L, c[1] + " describe " + describe(c[0]));
        }
        say(L, "lso " + fetchCommitted(tp, 0));
    }

    /** InitProducerId with the epoch a TV2 producer was handed by EndTxn (KIP-360). */
    static void reinit() throws IOException {
        String L = "reinit", tp = t("reinit"), tx = tp + "-tx";
        topic(tp, 1);
        Pe p = init(tx, 60000);
        produce((short) 12, tx, tp, 0, p, 0, true);
        Pe q = endOk(tx, p, true);
        say(L, "init v4 with handed epoch " + init(tx, 60000, (short) 4, q.pid, q.epoch));
        say(L, "init v4 with epoch before " + init(tx, 60000, (short) 4, p.pid, p.epoch));
        say(L, "describe " + describe(tx));
    }

    public static void main(String[] args) throws Exception {
        String broker = "127.0.0.1:39492";
        String only = null;
        boolean skipExhaust = false;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-broker" -> broker = args[++i];
                case "-only" -> only = args[++i];
                case "-skip-exhaust" -> skipExhaust = true;
                default -> throw new IllegalArgumentException(args[i]);
            }
        }
        String[] hp = broker.split(":");
        sock = new Socket(hp[0], Integer.parseInt(hp[1]));
        sock.setTcpNoDelay(true);
        in = new DataInputStream(new BufferedInputStream(sock.getInputStream()));
        out = new DataOutputStream(new BufferedOutputStream(sock.getOutputStream()));
        Map<String, ThrowingRunnable> all = new LinkedHashMap<>();
        all.put("features", Probe::features);
        all.put("basic", Probe::basic);
        all.put("table", Probe::table);
        all.put("unadded", Probe::unadded);
        all.put("refusals", Probe::refusals);
        all.put("offsets", Probe::offsets);
        all.put("batched", Probe::batched);
        all.put("timeout", Probe::timeout);
        all.put("zombie", Probe::zombie);
        all.put("reinit", Probe::reinit);
        all.put("owninit", Probe::owninit);
        all.put("exhaust", Probe::exhaust);
        for (var e : all.entrySet()) {
            if (only != null && !only.equals(e.getKey())) continue;
            if (skipExhaust && e.getKey().equals("exhaust")) continue;
            pids.clear();
            try {
                e.getValue().run();
            } catch (Exception ex) {
                say(e.getKey(), "EXCEPTION " + ex);
            }
        }
    }

    interface ThrowingRunnable { void run() throws Exception; }
}
