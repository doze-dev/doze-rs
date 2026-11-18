// Oracle probe for the last Kafka 4.4.0 versions (docs/port/behaviours/transactions.md TX-26 and TX-27): Produce v13 and
// TxnOffsetCommit v6 (topics named by id, KIP-516) and InitProducerId v6 (two-phase commit, KIP-939). Raw requests at
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
