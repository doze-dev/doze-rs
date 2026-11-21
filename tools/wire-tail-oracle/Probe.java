// Oracle probe for the last Kafka 4.4.0 versions (docs/port/behaviours/transactions.md TX-26 to TX-28, wire.md W37): Produce v13
// and TxnOffsetCommit v6 (topics named by id, KIP-516) and InitProducerId v6 (two-phase commit, KIP-939); and (debt K1,
// 2026-11-19) Fetch 12-18, ListOffsets 7-11, Metadata 13, OffsetCommit and OffsetFetch 10, FindCoordinator 4-6,
// DescribeGroups 6, DeleteGroups 3, ListTransactions 2, DescribeLogDirs 5 and ApiVersions 5 (scenarios `fetch` to
// `api-versions`, recorded in kafka-4.4.0.txt, the defaults; the other three files predate them). Raw requests at
// chosen versions over one plain connection, the answers printed one line each, with producer ids normalised (P0, P1, ...)
// and topic ids replaced by ID (the topic's), UNKNOWN (an id no topic has) and NIL (the zero id), so that two brokers'
// outputs can be diffed.
//
//   cd interop && java -cp "$(bun java/fetch.ts 4)" ../tools/wire-tail-oracle/Probe.java -broker host:port [-only name]
//
// What a real Apache Kafka 4.4.0 answered (KRaft, one node in combined mode, formatted with 4.4.0's defaults; the settings of
// tools/kafka-oracle/server.properties and transaction.abort.timed.out.transaction.cleanup.interval.ms=1000) is in three
// files, one per broker setting:
//   kafka-4.4.0.txt             the defaults (InitProducerId v6 is not offered: it is unstable in 4.4.0)
//   kafka-4.4.0-unstable.txt    unstable.api.versions.enable=true (v6 offered; transaction.two.phase.commit.enable false)
//   kafka-4.4.0-unstable-2pc.txt  both true
//   kafka-4.4.0-2pc.txt         transaction.two.phase.commit.enable=true alone (the same as the defaults: v6 is still not offered)
// doze-kafka run with the same settings (--unstable-api-versions, --transaction-two-phase-commit) answers the same lines but two
// kinds, both decided elsewhere: Produce is offered from v3, not v0 (the log stores record batch format v2 only; kafka-wire
// api.rs), and the all-apis lines list what the Kafka 4 profile offers, which is narrower than 4.4.0 in other APIs
// (tests/data/api-versions.json, recorded_api_versions.rs). Last compared 2026-11-18: no other line differs.
import org.apache.kafka.common.Uuid;
import org.apache.kafka.common.compress.Compression;
import org.apache.kafka.common.message.*;
import org.apache.kafka.common.protocol.ApiKeys;
import org.apache.kafka.common.protocol.ApiMessage;
import org.apache.kafka.common.record.internal.*;
import org.apache.kafka.common.requests.*;

import java.io.*;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.util.*;

public class Probe {
    static String host;
    static int port;
    static Socket sock;
    static DataInputStream in;
    static DataOutputStream out;
    static int corr = 0;
    static String run = Long.toString(System.currentTimeMillis() % 1_000_000, 36);
    static Map<Long, String> pids = new HashMap<>();
    static Map<Uuid, String> ids = new HashMap<>();
    static final Uuid UNKNOWN = new Uuid(0x0123_4567_89ab_cdefL, 0x0fed_cba9_8765_4321L);

    static String pid(long p) {
        if (p < 0) return Long.toString(p);
        return pids.computeIfAbsent(p, k -> "P" + pids.size());
    }

    static String id(Uuid u) {
        if (u.equals(Uuid.ZERO_UUID)) return "NIL";
        if (u.equals(UNKNOWN)) return "UNKNOWN";
        return ids.getOrDefault(u, "ID?");
    }

    static void connect() throws IOException {
        sock = new Socket(host, port);
        sock.setTcpNoDelay(true);
        in = new DataInputStream(new BufferedInputStream(sock.getInputStream()));
        out = new DataOutputStream(new BufferedOutputStream(sock.getOutputStream()));
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

    static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException e) { throw new RuntimeException(e); }
    }

    static String t(String s) { return "tail-" + run + "-" + s; }

    // ---- requests ----
    /** Makes a topic, waits until each partition answers, and returns its id (also known to `id` from then on as ID). */
    static Uuid topic(String name, int parts) throws IOException {
        var d = new CreateTopicsRequestData().setTimeoutMs(10000);
        d.topics().add(new CreateTopicsRequestData.CreatableTopic().setName(name).setNumPartitions(parts).setReplicationFactor((short) 1));
        var r = (CreateTopicsResponseData) ask(ApiKeys.CREATE_TOPICS, (short) 7, d);
        var made = r.topics().iterator().next();
        if (made.errorCode() != 0) throw new IllegalStateException("create " + name + ": " + made.errorCode());
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
        ids.put(made.topicId(), "ID");
        return made.topicId();
    }

    static void coordinator(byte type, String key) throws IOException {
        for (int i = 0; i < 100; i++) {
            var d = new FindCoordinatorRequestData().setKeyType(type).setCoordinatorKeys(List.of(key));
            var r = (FindCoordinatorResponseData) ask(ApiKeys.FIND_COORDINATOR, (short) 4, d);
            if (r.coordinators().get(0).errorCode() == 0) return;
            sleep(200);
        }
        throw new IllegalStateException("no coordinator for " + key);
    }

    record Pe(long pid, short epoch) {
        public String toString() { return Probe.pid(pid) + "/" + epoch; }
    }

    static InitProducerIdResponseData initRaw(short v, String tx, int timeout, boolean enable2Pc, boolean keep) throws IOException {
        var d = new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(timeout)
            .setProducerId(-1).setProducerEpoch((short) -1);
        if (v >= 6) d.setEnable2Pc(enable2Pc).setKeepPreparedTxn(keep);
        for (int i = 0; ; i++) {
            var r = (InitProducerIdResponseData) ask(ApiKeys.INIT_PRODUCER_ID, v, d);
            // Kafka answers CONCURRENT_TRANSACTIONS while it aborts what a previous owner left open, and the coordinator
            // codes while its transaction log is first loaded: ask again.
            if (List.of(14, 15, 16, 51).contains((int) r.errorCode()) && i < 100) { sleep(100); continue; }
            return r;
        }
    }

    static String initLine(short v, String tx, int timeout, boolean enable2Pc, boolean keep) throws IOException {
        var r = initRaw(v, tx, timeout, enable2Pc, keep);
        String s = r.errorCode() == 0 ? "ok " + new Pe(r.producerId(), r.producerEpoch()) : "error " + r.errorCode() + " " + r.producerId() + "/" + r.producerEpoch();
        if (v >= 6) s += " ongoing " + r.ongoingTxnProducerId() + "/" + r.ongoingTxnProducerEpoch();
        return s;
    }

    static Pe init(String tx, int timeout) throws IOException {
        coordinator((byte) 1, tx);
        var r = initRaw((short) 5, tx, timeout, false, false);
        if (r.errorCode() != 0) throw new IllegalStateException("init " + tx + ": " + r.errorCode());
        return new Pe(r.producerId(), r.producerEpoch());
    }

    record Target(Uuid id, String name, int part) {}

    /** One Produce of a batch per target (a topic entry each); a transactional one if `p` is given. Each partition's answer. */
    static String produce(short v, String tx, Pe p, Target... targets) throws IOException {
        var d = new ProduceRequestData().setAcks((short) -1).setTimeoutMs(10000).setTransactionalId(tx);
        var coll = new ProduceRequestData.TopicProduceDataCollection();
        for (Target tg : targets) {
            MemoryRecords recs = p == null
                ? MemoryRecords.withRecords(Compression.NONE, new SimpleRecord("v".getBytes()))
                : MemoryRecords.withTransactionalRecords(Compression.NONE, p.pid, p.epoch, 0, new SimpleRecord("v".getBytes()));
            var td = new ProduceRequestData.TopicProduceData().setName(v >= 13 ? "" : tg.name).setTopicId(v >= 13 ? tg.id : Uuid.ZERO_UUID);
            td.partitionData().add(new ProduceRequestData.PartitionProduceData().setIndex(tg.part).setRecords(recs));
            coll.add(td);
        }
        d.setTopicData(coll);
        var r = (ProduceResponseData) ask(ApiKeys.PRODUCE, v, d);
        List<String> each = new ArrayList<>();
        r.responses().forEach(t -> t.partitionResponses().forEach(pr -> each.add(
            (v >= 13 ? id(t.topicId()) : t.name().replace(run, "R")) + "-" + pr.index() + "=" + pr.errorCode()
                + (pr.errorCode() == 0 ? "@" + pr.baseOffset() : ""))));
        Collections.sort(each);
        return String.join(" ", each);
    }

    static String txnOffsets(short v, String tx, Pe p, String group, Target... targets) throws IOException {
        var d = new TxnOffsetCommitRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setGroupId(group)
            .setMemberId("").setGenerationIdOrMemberEpoch(-1);
        for (Target tg : targets) {
            var t = new TxnOffsetCommitRequestData.TxnOffsetCommitRequestTopic().setName(v >= 6 ? "" : tg.name).setTopicId(v >= 6 ? tg.id : Uuid.ZERO_UUID);
            t.partitions().add(new TxnOffsetCommitRequestData.TxnOffsetCommitRequestPartition().setPartitionIndex(tg.part).setCommittedOffset(7).setCommittedLeaderEpoch(-1));
            d.topics().add(t);
        }
        for (int i = 0; ; i++) {
            var r = (TxnOffsetCommitResponseData) ask(ApiKeys.TXN_OFFSET_COMMIT, v, d);
            List<String> each = new ArrayList<>();
            boolean concurrent = false;
            for (var t : r.topics()) for (var pr : t.partitions()) {
                // CONCURRENT_TRANSACTIONS, and the group coordinator while its offsets partition is first loaded: ask again.
                concurrent |= List.of(14, 15, 16, 51).contains((int) pr.errorCode());
                each.add((v >= 6 ? id(t.topicId()) : t.name().replace(run, "R")) + "-" + pr.partitionIndex() + "=" + pr.errorCode());
            }
            if (concurrent && i < 50) { sleep(100); continue; }
            Collections.sort(each);
            return String.join(" ", each);
        }
    }

    static String committed(String group, String topic, int part) throws IOException {
        var d = new OffsetFetchRequestData().setGroupId(group).setRequireStable(true);
        d.topics().add(new OffsetFetchRequestData.OffsetFetchRequestTopic().setName(topic).setPartitionIndexes(List.of(part)));
        for (int i = 0; ; i++) {
            var r = (OffsetFetchResponseData) ask(ApiKeys.OFFSET_FETCH, (short) 7, d);
            var pr = r.topics().get(0).partitions().get(0);
            // UNSTABLE_OFFSET_COMMIT while the commit's markers are written: ask again.
            if (pr.errorCode() == 88 && i < 50) { sleep(100); continue; }
            return pr.errorCode() == 0 ? "offset " + pr.committedOffset() : "error " + pr.errorCode();
        }
    }

    static String end(String tx, Pe p, boolean commit) throws IOException {
        var d = new EndTxnRequestData().setTransactionalId(tx).setProducerId(p.pid).setProducerEpoch(p.epoch).setCommitted(commit);
        for (int i = 0; ; i++) {
            var r = (EndTxnResponseData) ask(ApiKeys.END_TXN, (short) 5, d);
            if (r.errorCode() == 51 && i < 50) { sleep(100); continue; }
            if (r.errorCode() == 0) settled(tx);
            return r.errorCode() == 0 ? "ok " + new Pe(r.producerId(), r.producerEpoch()) : "error " + r.errorCode();
        }
    }

    static void settled(String tx) throws IOException {
        for (int i = 0; i < 100; i++) {
            if (!describe(tx).startsWith("Prepare")) return;
            sleep(50);
        }
    }

    static String describe(String tx) throws IOException {
        var d = new DescribeTransactionsRequestData().setTransactionalIds(List.of(tx));
        var r = (DescribeTransactionsResponseData) ask(ApiKeys.DESCRIBE_TRANSACTIONS, (short) 0, d);
        var s = r.transactionStates().get(0);
        if (s.errorCode() != 0) return "error " + s.errorCode();
        List<String> parts = new ArrayList<>();
        s.topics().forEach(t -> t.partitions().forEach(x -> parts.add(t.topic().replace(run, "R").replaceAll("__consumer_offsets", "OFFSETS") + "-" + x)));
        Collections.sort(parts);
        // The offsets partition a group maps to differs (50 partitions on both, but the hash is the group's): only "OFFSETS".
        List<String> shown = new ArrayList<>();
        for (String x : parts) shown.add(x.startsWith("OFFSETS") ? "OFFSETS" : x);
        return s.transactionState() + " " + new Pe(s.producerId(), s.producerEpoch()) + " timeout " + s.transactionTimeoutMs() + " " + shown;
    }

    // ---- scenarios ----
    static void apis() throws IOException {
        var d = new ApiVersionsRequestData().setClientSoftwareName("probe").setClientSoftwareVersion("1");
        var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, (short) 4, d);
        r.apiKeys().forEach(k -> {
            if (List.of(0, 22, 28).contains((int) k.apiKey())) say("apis", "api " + k.apiKey() + " " + k.minVersion() + ".." + k.maxVersion());
        });
    }

    /** Every API the broker lists (ApiVersions v0, as the recording of tests/data/api-versions.json was taken). */
    static void allApis() throws IOException {
        var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, (short) 0, new ApiVersionsRequestData());
        r.apiKeys().forEach(k -> say("all-apis", k.apiKey() + " " + k.minVersion() + ".." + k.maxVersion()));
    }

    /** Produce v13 by id: a known id, its partition out of range, an id no topic has, the nil id; then v12 by name. */
    static void produceIds() throws IOException {
        String L = "produce-ids", tp = t("p");
        Uuid id = topic(tp, 2);
        say(L, "v13 " + produce((short) 13, null, null, new Target(id, tp, 1), new Target(id, tp, 2), new Target(UNKNOWN, tp, 0), new Target(Uuid.ZERO_UUID, tp, 0)));
        say(L, "v13 again " + produce((short) 13, null, null, new Target(id, tp, 1)));
        say(L, "v12 " + produce((short) 12, null, null, new Target(id, tp, 1), new Target(UNKNOWN, t("nothing"), 0)));
    }

    /** A transactional Produce v13 adds the partitions its ids stand for; an unknown id's to nothing. */
    static void produceIdsTxn() throws IOException {
        String L = "produce-ids-txn", tp = t("pt"), tx = tp + "-tx";
        Uuid id = topic(tp, 2);
        Pe p = init(tx, 60000);
        say(L, "init " + p);
        say(L, "v13 " + produce((short) 13, tx, p, new Target(id, tp, 1), new Target(UNKNOWN, tp, 0)));
        say(L, "describe " + describe(tx));
        say(L, "end commit " + end(tx, p, true));
        say(L, "describe " + describe(tx));
    }

    /** TxnOffsetCommit v6 by id adds the group (as v5) and its offsets are applied by the commit; then v5 by name. */
    static void offsetsIds() throws IOException {
        String L = "offsets-ids", tp = t("o"), tx = tp + "-tx", g = tp + "-g";
        Uuid id = topic(tp, 1);
        Pe p = init(tx, 60000);
        coordinator((byte) 0, g);
        say(L, "v6 " + txnOffsets((short) 6, tx, p, g, new Target(id, tp, 0), new Target(id, tp, 3), new Target(UNKNOWN, tp, 0), new Target(Uuid.ZERO_UUID, tp, 0)));
        say(L, "describe " + describe(tx));
        Pe q;
        String e = end(tx, p, true);
        say(L, "end commit " + e);
        say(L, "committed " + committed(g, tp, 0));
        var r = initRaw((short) 5, tx, 60000, false, false);
        q = new Pe(r.producerId(), r.producerEpoch());
        say(L, "init again " + q);
        say(L, "v5 " + txnOffsets((short) 5, tx, q, g, new Target(id, tp, 0), new Target(UNKNOWN, t("nothing"), 0)));
        say(L, "end abort " + end(tx, q, false));
        say(L, "committed " + committed(g, tp, 0));
    }

    /** InitProducerId v6. Without the unstable versions the broker does not serve it at all (the connection is closed). */
    static void twoPhase() throws IOException {
        String L = "twopc", tx = t("2pc-tx");
        coordinator((byte) 1, tx);
        say(L, "v5 " + initLine((short) 5, tx, 60000, false, false));
        try {
            say(L, "v6 plain " + initLine((short) 6, tx, 60000, false, false));
        } catch (EOFException | java.net.SocketException e) {
            say(L, "v6 plain: connection closed");
            connect();
            return;
        }
        say(L, "v6 enable2Pc " + initLine((short) 6, tx, 60000, true, false));
        say(L, "describe " + describe(tx));
        say(L, "v6 enable2Pc timeout -1 " + initLine((short) 6, tx, -1, true, false));
        say(L, "v6 enable2Pc timeout 0 " + initLine((short) 6, tx, 0, true, false));
        say(L, "v6 enable2Pc timeout 16min " + initLine((short) 6, tx, 16 * 60 * 1000, true, false));
        say(L, "describe " + describe(tx));
        say(L, "v6 enable2Pc keep " + initLine((short) 6, tx, 60000, true, true));
        say(L, "v6 keep " + initLine((short) 6, tx, 60000, false, true));
        say(L, "v6 plain timeout -1 " + initLine((short) 6, tx, -1, false, false));
        say(L, "v6 plain " + initLine((short) 6, tx, 30000, false, false));
        say(L, "describe " + describe(tx));
        say(L, "v6 empty id enable2Pc keep " + initLine((short) 6, "", 1, true, true));
        var d = new InitProducerIdRequestData().setTransactionalId(null).setTransactionTimeoutMs(-1).setEnable2Pc(true).setKeepPreparedTxn(true);
        var r = (InitProducerIdResponseData) ask(ApiKeys.INIT_PRODUCER_ID, (short) 6, d);
        say(L, "v6 no id enable2Pc keep " + (r.errorCode() == 0 ? "ok epoch " + r.producerEpoch() : "error " + r.errorCode())
            + " ongoing " + r.ongoingTxnProducerId() + "/" + r.ongoingTxnProducerEpoch());
    }

    /** A two-phase commit takes over an open transaction as any owner; its own transaction outlives a short timeout. */
    static void twoPhaseOpen() throws IOException {
        String L = "twopc-open", tp = t("2o"), tx = tp + "-tx";
        Uuid id = topic(tp, 1);
        Pe p = init(tx, 60000);
        say(L, "init " + p);
        say(L, "produce " + produce((short) 13, tx, p, new Target(id, tp, 0)));
        InitProducerIdResponseData r;
        try {
            r = initRaw((short) 6, tx, 1000, true, false);
        } catch (EOFException | java.net.SocketException e) {
            say(L, "v6 enable2Pc: connection closed");
            connect();
            return;
        }
        if (r.errorCode() != 0) {
            say(L, "v6 enable2Pc error " + r.errorCode());
            return;
        }
        Pe q = new Pe(r.producerId(), r.producerEpoch());
        say(L, "v6 enable2Pc " + q);
        say(L, "describe " + describe(tx));
        say(L, "produce " + produce((short) 13, tx, q, new Target(id, tp, 0)));
        sleep(4000);
        say(L, "after 4 s " + describe(tx));
        say(L, "end abort " + end(tx, q, false));
        // The same with a plain InitProducerId and its 1 s timeout: aborted by the broker.
        Pe s = init(tx, 1000);
        say(L, "v5 init " + s);
        say(L, "produce " + produce((short) 13, tx, s, new Target(id, tp, 0)));
        sleep(4000);
        say(L, "after 4 s " + describe(tx));
    }

    // ---- the rest of 4.4.0's versions (debt K1, 2026-11-19) ----
    static int selfNode = -2;
    static String selfCluster = null;

    static String node(int n) { return n == selfNode ? "self" : Integer.toString(n); }

    /** The broker's own cluster id and node id, from Metadata v12. */
    static void self() throws IOException {
        if (selfCluster != null) return;
        var r = (MetadataResponseData) ask(ApiKeys.METADATA, (short) 12, new MetadataRequestData().setTopics(List.of()).setAllowAutoTopicCreation(false));
        selfCluster = r.clusterId();
        selfNode = r.brokers().iterator().next().nodeId();
    }

    static int count(BaseRecords recs) {
        int n = 0;
        if (recs instanceof MemoryRecords mr) for (var b : mr.batches()) for (var ignored : b) n++;
        return n;
    }

    static FetchResponseData fetchRaw(short v, Uuid id, String name, int part, long offset, int replica) throws IOException {
        var d = new FetchRequestData().setMaxWaitMs(0).setMinBytes(0).setMaxBytes(1 << 20).setIsolationLevel((byte) 0)
            .setSessionId(0).setSessionEpoch(-1);
        if (v >= 15) d.setReplicaState(new FetchRequestData.ReplicaState().setReplicaId(replica).setReplicaEpoch(-1));
        else d.setReplicaId(replica);
        var ft = new FetchRequestData.FetchTopic().setTopic(v >= 13 ? "" : name).setTopicId(v >= 13 ? id : Uuid.ZERO_UUID);
        ft.partitions().add(new FetchRequestData.FetchPartition().setPartition(part).setFetchOffset(offset).setPartitionMaxBytes(1 << 20)
            .setCurrentLeaderEpoch(-1).setLastFetchedEpoch(-1).setLogStartOffset(-1));
        d.topics().add(ft);
        return (FetchResponseData) ask(ApiKeys.FETCH, v, d);
    }

    static String fetchLine(short v, Uuid id, String name, int part, long offset) throws IOException {
        var r = fetchRaw(v, id, name, part, offset, -1);
        List<String> each = new ArrayList<>();
        each.add("top=" + r.errorCode() + " session=" + r.sessionId());
        for (var t : r.responses()) for (var p : t.partitions()) {
            String s = (v >= 13 ? id(t.topicId()) : t.topic().replace(run, "R")) + "-" + p.partitionIndex() + " err=" + p.errorCode()
                + " hw=" + p.highWatermark() + " lso=" + p.lastStableOffset() + " start=" + p.logStartOffset()
                + " records=" + count(p.records()) + " aborted=" + (p.abortedTransactions() == null ? "null" : p.abortedTransactions().size())
                + " preferred=" + p.preferredReadReplica();
            if (v >= 12) s += " diverging=" + p.divergingEpoch().epoch() + "/" + p.divergingEpoch().endOffset();
            if (v >= 12) s += " leader=" + p.currentLeader().leaderId() + "/" + p.currentLeader().leaderEpoch();
            each.add(s);
        }
        if (v >= 16) each.add("endpoints=" + r.nodeEndpoints().size());
        return String.join(" ", each);
    }

    /** Fetch v12 by name and v13 to v18 by id: the same answer at each; an id no topic has at v18. */
    static void fetchVersions() throws IOException {
        String L = "fetch", tp = t("f");
        Uuid id = topic(tp, 1);
        for (int i = 0; i < 3; i++) produce((short) 13, null, null, new Target(id, tp, 0));
        for (short v = 12; v <= 18; v++) say(L, "v" + v + " " + fetchLine(v, id, tp, 0, 1));
        say(L, "v18 past the end " + fetchLine((short) 18, id, tp, 0, 9));
        say(L, "v18 unknown id " + fetchLine((short) 18, UNKNOWN, tp, 0, 0));
        say(L, "v18 partition 5 " + fetchLine((short) 18, id, tp, 5, 0));
    }

    static String listLine(short v, String name, int part, long ts, byte isolation) throws IOException {
        var d = new ListOffsetsRequestData().setReplicaId(-1).setIsolationLevel(isolation);
        if (v >= 10) d.setTimeoutMs(1000);
        var lt = new ListOffsetsRequestData.ListOffsetsTopic().setName(name);
        lt.partitions().add(new ListOffsetsRequestData.ListOffsetsPartition().setPartitionIndex(part).setTimestamp(ts).setCurrentLeaderEpoch(-1));
        d.topics().add(lt);
        var r = (ListOffsetsResponseData) ask(ApiKeys.LIST_OFFSETS, v, d);
        var p = r.topics().get(0).partitions().get(0);
        return "err=" + p.errorCode() + " offset=" + p.offset() + " epoch=" + p.leaderEpoch() + " time=" + (p.timestamp() >= 0 ? "T" : Long.toString(p.timestamp()));
    }

    /** ListOffsets v7 to v11 with every special time, on a topic with two records and on one with none. */
    static void listOffsetsVersions() throws IOException {
        String L = "list-offsets", tp = t("l"), empty = t("le");
        Uuid id = topic(tp, 1);
        topic(empty, 1);
        produce((short) 13, null, null, new Target(id, tp, 0));
        produce((short) 13, null, null, new Target(id, tp, 0));
        long[] times = {-1, -2, -3, -4, -5, -6, -7, 0};
        for (short v = 7; v <= 11; v++) {
            for (long ts : times) say(L, "v" + v + " " + ts + " " + listLine(v, tp, 0, ts, (byte) 0));
        }
        for (long ts : new long[] {-1, -2, -4, -5, -6}) say(L, "v11 empty " + ts + " " + listLine((short) 11, empty, 0, ts, (byte) 0));
        for (long ts : new long[] {-1, -4, -5, -6}) say(L, "v11 committed " + ts + " " + listLine((short) 11, tp, 0, ts, (byte) 1));
        say(L, "v10 unknown topic -6 " + listLine((short) 10, t("nothing"), 0, -6, (byte) 0));
        say(L, "v11 unknown topic -6 " + listLine((short) 11, t("nothing"), 0, -6, (byte) 0));
        say(L, "v11 partition 3 -5 " + listLine((short) 11, tp, 3, -5, (byte) 0));
    }

    /** Metadata v12 and v13: the top-level error code (v13) for a known topic, an unknown one, and all topics. */
    static void metadataVersions() throws IOException {
        String L = "metadata", tp = t("m");
        Uuid id = topic(tp, 2);
        for (short v = 12; v <= 13; v++) {
            for (boolean byId : new boolean[] {false, true}) {
                var d = new MetadataRequestData().setAllowAutoTopicCreation(false);
                if (byId) {
                    d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(null).setTopicId(id));
                    d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(null).setTopicId(UNKNOWN));
                } else {
                    d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(tp));
                    d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(t("nothing")));
                }
                var r = (MetadataResponseData) ask(ApiKeys.METADATA, v, d);
                List<String> each = new ArrayList<>();
                for (var t : r.topics()) each.add((t.name() == null ? "null" : t.name().replace(run, "R")) + "/" + id(t.topicId()) + "=" + t.errorCode() + ":" + t.partitions().size());
                Collections.sort(each);
                say(L, "v" + v + (byId ? " ids" : " names") + " top=" + (v >= 13 ? r.errorCode() : "-") + " " + String.join(" ", each));
            }
        }
    }

    static String commit10(short v, String g, Uuid id, String name, int... parts) throws IOException {
        var d = new OffsetCommitRequestData().setGroupId(g).setMemberId("").setGenerationIdOrMemberEpoch(-1);
        var t = new OffsetCommitRequestData.OffsetCommitRequestTopic().setName(v >= 10 ? "" : name).setTopicId(v >= 10 ? id : Uuid.ZERO_UUID);
        for (int p : parts) t.partitions().add(new OffsetCommitRequestData.OffsetCommitRequestPartition().setPartitionIndex(p).setCommittedOffset(5 + p).setCommittedLeaderEpoch(-1).setCommittedMetadata("m" + p));
        d.topics().add(t);
        var r = (OffsetCommitResponseData) ask(ApiKeys.OFFSET_COMMIT, v, d);
        List<String> each = new ArrayList<>();
        for (var rt : r.topics()) for (var p : rt.partitions())
            each.add((v >= 10 ? id(rt.topicId()) : rt.name().replace(run, "R")) + "-" + p.partitionIndex() + "=" + p.errorCode());
        return String.join(" ", each);
    }

    static String fetch10(short v, String g, List<Uuid> ids, List<String> names) throws IOException {
        var grp = new OffsetFetchRequestData.OffsetFetchRequestGroup().setGroupId(g);
        if (ids == null && names == null) grp.setTopics(null);
        else if (v >= 10) for (Uuid u : ids) grp.topics().add(new OffsetFetchRequestData.OffsetFetchRequestTopics().setTopicId(u).setPartitionIndexes(List.of(0, 1)));
        else for (String n : names) grp.topics().add(new OffsetFetchRequestData.OffsetFetchRequestTopics().setName(n).setPartitionIndexes(List.of(0, 1)));
        var d = new OffsetFetchRequestData().setRequireStable(true).setGroups(List.of(grp));
        var r = (OffsetFetchResponseData) ask(ApiKeys.OFFSET_FETCH, v, d);
        var rg = r.groups().get(0);
        List<String> each = new ArrayList<>();
        each.add("group=" + rg.errorCode());
        for (var rt : rg.topics()) for (var p : rt.partitions())
            each.add((v >= 10 ? id(rt.topicId()) : rt.name().replace(run, "R")) + "-" + p.partitionIndex() + "=" + p.errorCode() + "@" + p.committedOffset() + "/" + p.metadata());
        return String.join(" ", each);
    }

    /** OffsetCommit v10 and OffsetFetch v10 name topics by id: a known id, its partition out of range, an id no topic has. */
    static void offsetsByIdVersions() throws IOException {
        String L = "offsets-10", tp = t("c"), g = tp + "-g";
        Uuid id = topic(tp, 2);
        coordinator((byte) 0, g);
        say(L, "commit v10 " + commit10((short) 10, g, id, tp, 0, 1, 2));
        say(L, "commit v10 unknown id " + commit10((short) 10, g, UNKNOWN, tp, 0));
        say(L, "commit v10 nil id " + commit10((short) 10, g, Uuid.ZERO_UUID, tp, 0));
        say(L, "commit v9 " + commit10((short) 9, g, id, tp, 1));
        say(L, "fetch v10 " + fetch10((short) 10, g, List.of(id, UNKNOWN), null));
        say(L, "fetch v10 all " + fetch10((short) 10, g, null, null));
        say(L, "fetch v9 " + fetch10((short) 9, g, null, List.of(tp, t("nothing"))));
        say(L, "fetch v10 no group " + fetch10((short) 10, g + "-none", List.of(id), null));
    }

    static String coordLine(short v, byte type, String... keys) throws IOException {
        var d = new FindCoordinatorRequestData().setKeyType(type).setCoordinatorKeys(List.of(keys));
        FindCoordinatorResponseData r = null;
        // Kafka makes __share_group_state at the first share key and answers COORDINATOR_NOT_AVAILABLE meanwhile: ask again.
        for (int i = 0; i < 100; i++) {
            r = (FindCoordinatorResponseData) ask(ApiKeys.FIND_COORDINATOR, v, d);
            if (r.coordinators().stream().noneMatch(c -> c.errorCode() == 15)) break;
            sleep(200);
        }
        List<String> each = new ArrayList<>();
        for (var c : r.coordinators())
            each.add("[" + c.key().replace(run, "R") + "]=" + c.errorCode() + " node=" + node(c.nodeId()) + " msg=" + c.errorMessage());
        return String.join(" ", each);
    }

    /** FindCoordinator v5 and v6: v6 adds the share coordinator's key type (2), a key `group:topicId:partition`. */
    static void findCoordinatorVersions() throws IOException {
        self();
        String L = "find-coordinator", g = t("fc-g");
        // Fixed topic ids, so that two runs' lines compare.
        String id1 = "0bFg2GHCR_67Zo0_m2MTGQ", id2 = "ZJXISKCaTLSUkskfFuyhqQ";
        String good = g + ":" + id1 + ":0";
        for (short v = 4; v <= 6; v++) say(L, "v" + v + " group " + coordLine(v, (byte) 0, g));
        say(L, "v5 share " + coordLine((short) 5, (byte) 2, good));
        say(L, "v6 share " + coordLine((short) 6, (byte) 2, good));
        say(L, "v6 share colons in the group " + coordLine((short) 6, (byte) 2, "a:b:" + id2 + ":3"));
        say(L, "v6 share bad keys " + coordLine((short) 6, (byte) 2, "x", "g:notanid:0", g + ":" + id1 + ":x", " :" + id1 + ":0", "", g + ":" + id2 + ":-1"));
        say(L, "v6 key type 3 " + coordLine((short) 6, (byte) 3, g));
    }

    static String describeLine(short v, String... groups) throws IOException {
        var d = new DescribeGroupsRequestData().setGroups(List.of(groups));
        var r = (DescribeGroupsResponseData) ask(ApiKeys.DESCRIBE_GROUPS, v, d);
        List<String> each = new ArrayList<>();
        for (var x : r.groups())
            each.add("[" + x.groupId().replace(run, "R") + "]=" + x.errorCode() + " " + x.groupState() + " type=" + x.protocolType() + (v >= 6 ? " msg=" + (x.errorMessage() == null ? "null" : x.errorMessage().replace(run, "R")) : ""));
        // Sorted: Kafka answers in the order of each group's offsets partition, which follows the run's random names
        // (the order itself is held by groups::order::tests).
        Collections.sort(each);
        return String.join(" ", each);
    }

    /** DescribeGroups v5 and v6: a group not found, an Empty classic group, a consumer group (KIP-848). */
    static void describeGroupsVersions() throws IOException {
        String L = "describe-groups", tp = t("dg"), empty = tp + "-empty", nothing = tp + "-none", consumer = tp + "-848";
        Uuid id = topic(tp, 1);
        coordinator((byte) 0, empty);
        coordinator((byte) 0, consumer);
        commit10((short) 10, empty, id, tp, 0);
        var hb = new ConsumerGroupHeartbeatRequestData().setGroupId(consumer).setMemberId(Uuid.randomUuid().toString()).setMemberEpoch(0)
            .setRebalanceTimeoutMs(30000).setSubscribedTopicNames(List.of(tp)).setTopicPartitions(List.of());
        var hr = (ConsumerGroupHeartbeatResponseData) ask(ApiKeys.CONSUMER_GROUP_HEARTBEAT, (short) 1, hb);
        say(L, "heartbeat " + hr.errorCode());
        for (short v = 5; v <= 6; v++) say(L, "v" + v + " " + describeLine(v, nothing, empty, consumer, ""));
    }

    static String deleteLine(short v, String... groups) throws IOException {
        var d = new DeleteGroupsRequestData().setGroupsNames(List.of(groups));
        var r = (DeleteGroupsResponseData) ask(ApiKeys.DELETE_GROUPS, v, d);
        List<String> each = new ArrayList<>();
        for (var x : r.results()) each.add("[" + x.groupId().replace(run, "R") + "]=" + x.errorCode() + (v >= 3 ? " msg=" + x.errorMessage() : ""));
        Collections.sort(each);
        return String.join(" ", each);
    }

    /** DeleteGroups v2 and v3 (v3 adds an error message to each result). */
    static void deleteGroupsVersions() throws IOException {
        String L = "delete-groups", tp = t("del"), a = tp + "-a", b = tp + "-b";
        Uuid id = topic(tp, 1);
        coordinator((byte) 0, a);
        coordinator((byte) 0, b);
        commit10((short) 10, a, id, tp, 0);
        commit10((short) 10, b, id, tp, 0);
        say(L, "v2 " + deleteLine((short) 2, a, tp + "-none"));
        say(L, "v3 " + deleteLine((short) 3, b, tp + "-none", ""));
    }

    static String listTxnLine(short v, String pattern) throws IOException {
        var d = new ListTransactionsRequestData();
        if (v >= 2) d.setTransactionalIdPattern(pattern);
        var r = (ListTransactionsResponseData) ask(ApiKeys.LIST_TRANSACTIONS, v, d);
        List<String> each = new ArrayList<>();
        for (var x : r.transactionStates()) if (x.transactionalId().contains(run)) each.add(x.transactionalId().replace(run, "R") + ":" + x.transactionState());
        Collections.sort(each);
        return "err=" + r.errorCode() + " " + String.join(" ", each);
    }

    /** ListTransactions v1 and v2 (v2: the transactional ids that match a pattern, KIP-1152). */
    static void listTransactionsVersions() throws IOException {
        String L = "list-transactions", a = t("lt-alpha"), b = t("lt-beta");
        init(a, 60000);
        init(b, 60000);
        say(L, "v1 " + listTxnLine((short) 1, null));
        say(L, "v2 null " + listTxnLine((short) 2, null));
        say(L, "v2 empty " + listTxnLine((short) 2, ""));
        say(L, "v2 alpha " + listTxnLine((short) 2, ".*lt-alph."));
        say(L, "v2 part " + listTxnLine((short) 2, "lt-alpha"));
        say(L, "v2 either " + listTxnLine((short) 2, "tail-.*-lt-(alpha|beta)"));
        say(L, "v2 bad " + listTxnLine((short) 2, "(unclosed"));
    }

    /** DescribeLogDirs v4 and v5 (v5 adds IsCordoned to each directory). */
    static void logDirsVersions() throws IOException {
        String L = "log-dirs", tp = t("ld");
        topic(tp, 2);
        for (short v = 4; v <= 5; v++) {
            var d = new DescribeLogDirsRequestData();
            var dt = new DescribeLogDirsRequestData.DescribableLogDirTopic().setTopic(tp).setPartitions(List.of(0, 1));
            d.topics().add(dt);
            var r = (DescribeLogDirsResponseData) ask(ApiKeys.DESCRIBE_LOG_DIRS, v, d);
            List<String> each = new ArrayList<>();
            each.add("top=" + r.errorCode() + " dirs=" + r.results().size());
            for (var x : r.results()) {
                List<String> parts = new ArrayList<>();
                x.topics().forEach(t -> t.partitions().forEach(p -> parts.add(t.name().replace(run, "R") + "-" + p.partitionIndex() + ":" + p.partitionSize() + "/" + p.offsetLag() + "/" + p.isFutureKey())));
                Collections.sort(parts);
                each.add("err=" + x.errorCode() + (v >= 5 ? " cordoned=" + x.isCordoned() : "") + " total=" + (x.totalBytes() > 0 ? "N" : Long.toString(x.totalBytes())) + " usable=" + (x.usableBytes() > 0 ? "N" : Long.toString(x.usableBytes())) + " " + parts);
            }
            say(L, "v" + v + " " + String.join(" ", each));
        }
    }

    static String versionsLine(short v, String cluster, int nodeId) throws IOException {
        var d = new ApiVersionsRequestData().setClientSoftwareName("probe").setClientSoftwareVersion("1");
        if (v >= 5) d.setClusterId(cluster).setNodeId(nodeId);
        var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, v, d);
        var own = r.apiKeys().find((short) 18);
        return "err=" + r.errorCode() + " rows=" + (r.apiKeys().isEmpty() ? 0 : "some") + " apiversions=" + (own == null ? "none" : own.minVersion() + ".." + own.maxVersion());
    }

    /** ApiVersions v5 (KIP-1242): a client may say which cluster and node it means; another is REBOOTSTRAP_REQUIRED. */
    static void apiVersionsFive() throws IOException {
        self();
        String L = "api-versions";
        say(L, "v4 " + versionsLine((short) 4, null, -1));
        say(L, "v5 none " + versionsLine((short) 5, null, -1));
        say(L, "v5 this cluster and node " + versionsLine((short) 5, selfCluster, selfNode));
        say(L, "v5 this cluster, node -1 " + versionsLine((short) 5, selfCluster, -1));
        say(L, "v5 this cluster, another node " + versionsLine((short) 5, selfCluster, selfNode + 7));
        say(L, "v5 another cluster " + versionsLine((short) 5, "AAAAAAAAAAAAAAAAAAAAAA", selfNode));
        say(L, "v5 no cluster, another node " + versionsLine((short) 5, null, selfNode + 7));
        say(L, "v5 empty cluster " + versionsLine((short) 5, "", selfNode));
        say(L, "after them " + versionsLine((short) 4, null, -1));
    }

    public static void main(String[] args) throws Exception {
        String broker = "127.0.0.1:39700";
        String only = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-broker" -> broker = args[++i];
                case "-only" -> only = args[++i];
                default -> throw new IllegalArgumentException(args[i]);
            }
        }
        String[] hp = broker.split(":");
        host = hp[0];
        port = Integer.parseInt(hp[1]);
        connect();
        Map<String, ThrowingRunnable> all = new LinkedHashMap<>();
        all.put("apis", Probe::apis);
        all.put("all-apis", Probe::allApis);
        all.put("produce-ids", Probe::produceIds);
        all.put("produce-ids-txn", Probe::produceIdsTxn);
        all.put("offsets-ids", Probe::offsetsIds);
        all.put("twopc", Probe::twoPhase);
        all.put("twopc-open", Probe::twoPhaseOpen);
        all.put("fetch", Probe::fetchVersions);
        all.put("list-offsets", Probe::listOffsetsVersions);
        all.put("metadata", Probe::metadataVersions);
        all.put("offsets-10", Probe::offsetsByIdVersions);
        all.put("find-coordinator", Probe::findCoordinatorVersions);
        all.put("describe-groups", Probe::describeGroupsVersions);
        all.put("delete-groups", Probe::deleteGroupsVersions);
        all.put("list-transactions", Probe::listTransactionsVersions);
        all.put("log-dirs", Probe::logDirsVersions);
        all.put("api-versions", Probe::apiVersionsFive);
        for (var e : all.entrySet()) {
            if (only != null && !only.equals(e.getKey())) continue;
            pids.clear();
            try {
                e.getValue().run();
            } catch (Exception ex) {
                say(e.getKey(), "EXCEPTION " + ex);
                try { connect(); } catch (IOException ignored) { }
            }
        }
    }

    interface ThrowingRunnable { void run() throws Exception; }
}
