// Oracle probe for the older profiles (debt K2, 2026-11-19): `--kafka-version 1`, `2` and `3` stand for Kafka 1.1, 2.8 and 3.9,
// and serve every version of every API the recorded broker of that major lists (inter-broker APIs aside, K4). Raw requests at
// chosen versions over one plain connection, written with kafka-clients 3.9.1 (the last client that writes every version those
// brokers serve, v0 included, and builds message sets of format v0 and v1), the answers printed one line each and normalised
// (topic and producer ids, member ids, timestamps, the cluster id) so that two brokers' outputs can be diffed.
//
//   cd interop && java -cp "$(bun java/fetch.ts 3.9)" ../tools/old-profiles-oracle/Probe.java -broker host:port -major N [-only name]
//
// `-major` says which release the broker is (1, 2 or 3): every scenario asks only for versions that release lists. What the real
// brokers answered is in `kafka-1.1.txt` (confluentinc/cp-kafka:4.1.4 with ZooKeeper), `kafka-2.8.txt` (cp-kafka:6.2.15 with
// ZooKeeper) and `kafka-3.9.txt` (apache/kafka:3.9.1, KRaft combined mode): the images `crates/kafka-wire/tests/data/api-versions.json`
// was recorded from, run by `brokers.sh` with nothing configured but single-node replication factors and no initial rebalance
// delay. `compare.sh` runs doze-kafka at each profile and diffs.
import org.apache.kafka.common.Uuid;
import org.apache.kafka.common.compress.Compression;
import org.apache.kafka.common.message.*;
import org.apache.kafka.common.protocol.ApiKeys;
import org.apache.kafka.common.protocol.ApiMessage;
import org.apache.kafka.common.record.*;
import org.apache.kafka.common.requests.*;
import org.apache.kafka.common.header.internals.RecordHeader;
import org.apache.kafka.common.header.Header;

import java.io.*;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.regex.*;

public class Probe {
    static String host;
    static int port;
    static int major;
    static Socket sock;
    static DataInputStream in;
    static DataOutputStream out;
    static int corr = 0;
    static String run = Long.toString(System.currentTimeMillis() % 1_000_000, 36);
    /** The records' timestamps: a minute ago, rounded to the second (a broker rolls a segment whose first timestamp is a week old,
     *  so old timestamps would cut every read at a segment). */
    static final long BASE = (System.currentTimeMillis() - 60_000) / 1000 * 1000;
    static Map<Long, String> pids = new HashMap<>();
    static Map<Uuid, String> ids = new HashMap<>();
    /** Each API's highest version at the major under test, from its ApiVersions answer. */
    static Map<Integer, short[]> listed = new TreeMap<>();

    static void connect() throws IOException {
        sock = new Socket(host, port);
        sock.setTcpNoDelay(true);
        sock.setSoTimeout(30000);
        in = new DataInputStream(new BufferedInputStream(sock.getInputStream()));
        out = new DataOutputStream(new BufferedOutputStream(sock.getOutputStream()));
    }

    static ByteBuffer askRaw(ApiKeys key, short v, ApiMessage data) throws IOException {
        RequestHeader h = new RequestHeader(key, v, "probe", ++corr);
        ByteBuffer b = RequestUtils.serialize(h.data(), h.headerVersion(), data, v);
        out.writeInt(b.remaining());
        out.write(b.array(), b.arrayOffset() + b.position(), b.remaining());
        out.flush();
        int n = in.readInt();
        byte[] r = new byte[n];
        in.readFully(r);
        return ByteBuffer.wrap(r);
    }

    static ApiMessage ask(ApiKeys key, short v, ApiMessage data) throws IOException {
        RequestHeader h = new RequestHeader(key, v, "probe", corr + 1);
        ByteBuffer r = askRaw(key, v, data);
        return AbstractResponse.parseResponse(r, h).data();
    }

    /** Sends a request with acks=0 and no answer. */
    static void tell(ApiKeys key, short v, ApiMessage data) throws IOException {
        RequestHeader h = new RequestHeader(key, v, "probe", ++corr);
        ByteBuffer b = RequestUtils.serialize(h.data(), h.headerVersion(), data, v);
        out.writeInt(b.remaining());
        out.write(b.array(), b.arrayOffset() + b.position(), b.remaining());
        out.flush();
    }

    static void say(String label, String what) {
        System.out.println(label + ": " + what);
    }

    static void sleep(long ms) {
        try { Thread.sleep(ms); } catch (InterruptedException e) { throw new RuntimeException(e); }
    }

    static String t(String s) { return "old-" + run + "-" + s; }

    interface Line { String get() throws IOException; }

    /** The line, or `closed` if the broker dropped the connection instead of answering (then connected again). */
    static String guarded(Line l) throws IOException {
        try {
            return l.get();
        } catch (EOFException | java.net.SocketException e) {
            connect();
            return "closed";
        }
    }

    static short max(ApiKeys k) {
        short[] r = listed.get((int) k.id);
        return r == null ? -1 : r[1];
    }

    static short min(ApiKeys k) {
        short[] r = listed.get((int) k.id);
        return r == null ? -1 : r[0];
    }

    static boolean has(ApiKeys k) { return listed.containsKey((int) k.id); }

    // Normalising: what a broker makes up is replaced by a placeholder.
    static final Pattern UUIDISH = Pattern.compile("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}");
    static final Pattern TS = Pattern.compile("(?<![0-9])1[0-9]{12}(?![0-9])");

    static String norm(Object o) {
        String s = String.valueOf(o);
        s = s.replace(run, "R");
        s = UUIDISH.matcher(s).replaceAll("UUID");
        s = TS.matcher(s).replaceAll("T");
        s = s.replaceAll("topicId=[A-Za-z0-9_-]{22}", "topicId=ID");
        s = s.replaceAll("clusterId='[^']*'", "clusterId=C");
        s = s.replaceAll("producerId=[0-9]+", "producerId=P");
        s = s.replaceAll("throttleTimeMs=[0-9]+", "throttleTimeMs=_");
        return s;
    }

    // ---- requests ----
    /** Makes a topic with CreateTopics at the newest version the broker lists, waits until each partition answers. */
    static void topic(String name, int parts, Map<String, String> configs) throws IOException {
        short v = (short) Math.min(max(ApiKeys.CREATE_TOPICS), 4);
        var d = new CreateTopicsRequestData().setTimeoutMs(10000);
        var ct = new CreateTopicsRequestData.CreatableTopic().setName(name).setNumPartitions(parts).setReplicationFactor((short) 1);
        configs.forEach((k, val) -> ct.configs().add(new CreateTopicsRequestData.CreatableTopicConfig().setName(k).setValue(val)));
        d.topics().add(ct);
        var r = (CreateTopicsResponseData) ask(ApiKeys.CREATE_TOPICS, v, d);
        var made = r.topics().iterator().next();
        if (made.errorCode() != 0) throw new IllegalStateException("create " + name + ": " + made.errorCode() + " " + made.errorMessage());
        for (int part = 0; part < parts; part++) {
            for (int i = 0; ; i++) {
                var lo = new ListOffsetsRequestData().setReplicaId(-1).setIsolationLevel((byte) 0);
                var lt = new ListOffsetsRequestData.ListOffsetsTopic().setName(name);
                lt.partitions().add(new ListOffsetsRequestData.ListOffsetsPartition().setPartitionIndex(part).setTimestamp(-1).setCurrentLeaderEpoch(-1));
                lo.topics().add(lt);
                var lr = (ListOffsetsResponseData) ask(ApiKeys.LIST_OFFSETS, (short) 1, lo);
                if (lr.topics().get(0).partitions().get(0).errorCode() == 0) break;
                if (i > 100) throw new IllegalStateException("partition " + name + "-" + part + " never hosted");
                sleep(100);
            }
        }
    }

    static void topic(String name, int parts) throws IOException { topic(name, parts, Map.of()); }

    static void coordinator(byte type, String key) throws IOException {
        for (int i = 0; i < 150; i++) {
            var d = new FindCoordinatorRequestData().setKey(key).setKeyType(type);
            var r = (FindCoordinatorResponseData) ask(ApiKeys.FIND_COORDINATOR, (short) 1, d);
            if (r.errorCode() == 0) {
                if (type == 0) groupReady(key);
                return;
            }
            sleep(200);
        }
        throw new IllegalStateException("no coordinator for " + key);
    }

    /** Waits until the group's coordinator has loaded its offsets partition (a KRaft broker names it before it has). */
    static void groupReady(String group) throws IOException {
        for (int i = 0; i < 150; i++) {
            var r = (OffsetFetchResponseData) ask(ApiKeys.OFFSET_FETCH, (short) 2, new OffsetFetchRequestData().setGroupId(group).setTopics(null));
            if (r.errorCode() == 0) return;
            sleep(200);
        }
        throw new IllegalStateException("group coordinator for " + group + " never loaded");
    }

    /** One Produce of `records` at `v` to `topic`-`part`: the partition's answer. */
    static String produce(short v, String topic, int part, ByteBuffer records, String tx) throws IOException {
        var d = new ProduceRequestData().setAcks((short) -1).setTimeoutMs(10000);
        if (v >= 3) d.setTransactionalId(tx);
        var coll = new ProduceRequestData.TopicProduceDataCollection();
        var td = new ProduceRequestData.TopicProduceData().setName(topic);
        td.partitionData().add(new ProduceRequestData.PartitionProduceData().setIndex(part).setRecords(MemoryRecords.readableRecords(records.duplicate())));
        coll.add(td);
        d.setTopicData(coll);
        var r = (ProduceResponseData) ask(ApiKeys.PRODUCE, v, d);
        var pr = r.responses().iterator().next().partitionResponses().get(0);
        String s = "err=" + pr.errorCode() + " base=" + pr.baseOffset();
        if (v >= 2) s += " appendTime=" + (pr.logAppendTimeMs() > 0 ? "T" : Long.toString(pr.logAppendTimeMs()));
        if (v >= 5) s += " logStart=" + pr.logStartOffset();
        if (v >= 8) {
            s += " recordErrors=" + pr.recordErrors().size();
            if (pr.errorMessage() != null) s += " msg=" + pr.errorMessage().replace(run, "R");
        }
        return s;
    }

    static String produce(short v, String topic, ByteBuffer records) throws IOException { return produce(v, topic, 0, records, null); }

    /** A message set or batch of `magic` and `compression` with `n` records, keys k0.. and values v0.., timestamps base+i. */
    static ByteBuffer records(byte magic, Compression c, int n, long base, TimestampType type) {
        ByteBuffer buf = ByteBuffer.allocate(64 * 1024);
        MemoryRecordsBuilder b = MemoryRecords.builder(buf, magic, c, type, 0L, base);
        for (int i = 0; i < n; i++) {
            if (magic >= 2) {
                b.append(new SimpleRecord(base + i, ("k" + i).getBytes(), ("v" + i).getBytes(), new Header[] {new RecordHeader("h", "x".getBytes())}));
            } else {
                b.append(base + i, ("k" + i).getBytes(), ("v" + i).getBytes());
            }
        }
        return b.build().buffer();
    }

    static ByteBuffer records(byte magic, Compression c, int n) { return records(magic, c, n, BASE, TimestampType.CREATE_TIME); }

    static ByteBuffer concat(ByteBuffer... parts) {
        int n = 0;
        for (ByteBuffer p : parts) n += p.remaining();
        ByteBuffer b = ByteBuffer.allocate(n);
        for (ByteBuffer p : parts) b.put(p.duplicate());
        b.flip();
        return b;
    }

    /** Fetch `topic`-0 from `offset` at `v`: the partition's codes and offsets, and every batch and record in the answer. */
    static String fetch(short v, String topic, long offset, int maxBytes) throws IOException {
        var d = new FetchRequestData().setReplicaId(-1).setMaxWaitMs(0).setMinBytes(0).setMaxBytes(maxBytes).setIsolationLevel((byte) 0);
        var ft = new FetchRequestData.FetchTopic().setTopic(topic);
        ft.partitions().add(new FetchRequestData.FetchPartition().setPartition(0).setFetchOffset(offset).setPartitionMaxBytes(maxBytes).setLogStartOffset(-1).setCurrentLeaderEpoch(-1));
        d.topics().add(ft);
        var r = (FetchResponseData) ask(ApiKeys.FETCH, v, d);
        var p = r.responses().get(0).partitions().get(0);
        StringBuilder s = new StringBuilder("err=" + p.errorCode() + " hw=" + p.highWatermark());
        if (v >= 4) s.append(" lso=").append(p.lastStableOffset()).append(" aborted=").append(p.abortedTransactions() == null ? "null" : p.abortedTransactions().size());
        if (v >= 5) s.append(" logStart=").append(p.logStartOffset());
        s.append(" ").append(describe((BaseRecords) p.records()));
        return s.toString();
    }

    static String describe(BaseRecords base) {
        if (base == null) return "records=null";
        MemoryRecords recs = (MemoryRecords) base;
        StringBuilder s = new StringBuilder("bytes=" + recs.sizeInBytes());
        ByteBuffer raw = recs.buffer().duplicate();
        try {
            for (RecordBatch b : recs.batches()) {
                s.append(" [m").append(b.magic()).append(" ").append(b.compressionType().name).append(" ")
                    .append(b.baseOffset()).append("-").append(b.lastOffset())
                    .append(" ").append(b.timestampType()).append(" max=").append(ts(b.maxTimestamp()));
                if (b.magic() >= 2) s.append(" pid=").append(b.producerId() < 0 ? "-1" : "P").append(" txn=").append(b.isTransactional()).append(" ctl=").append(b.isControlBatch()).append(" seq=").append(b.baseSequence());
                s.append(" valid=").append(b.isValid());
                for (org.apache.kafka.common.record.Record rec : b) {
                    s.append(" (").append(rec.offset()).append(" ").append(ts(rec.timestamp())).append(" ")
                        .append(str(rec.key())).append("=").append(str(rec.value()));
                    if (rec.headers().length > 0) s.append(" h").append(rec.headers().length);
                    s.append(")");
                }
                s.append("]");
            }
        } catch (Exception e) {
            s.append(" !").append(e.getClass().getSimpleName());
        }
        // What follows the last whole batch (a partial one, or a down-conversion's filler).
        int whole = 0;
        ByteBuffer d = raw.duplicate();
        while (d.remaining() >= 12) {
            int len = d.getInt(d.position() + 8);
            if (len < 0 || d.remaining() < 12 + len) break;
            whole += 12 + len;
            d.position(d.position() + 12 + len);
        }
        if (whole < recs.sizeInBytes()) {
            ByteBuffer tail = raw.duplicate();
            tail.position(whole);
            s.append(" tail=").append(recs.sizeInBytes() - whole);
            if (tail.remaining() >= 12) s.append(" tailOffset=").append(tail.getLong(whole)).append(" tailLen=").append(tail.getInt(whole + 8));
        }
        return s.toString();
    }

    /** A time near BASE as its distance from it (B+n), another recent one as T, anything else as it is. */
    static String ts(long t) {
        if (Math.abs(t - BASE) < 86_400_000L) return "B" + (t >= BASE ? "+" : "") + (t - BASE);
        return t > 1_700_000_000_000L ? "T" : Long.toString(t);
    }

    static String str(ByteBuffer b) {
        if (b == null) return "null";
        byte[] a = new byte[b.remaining()];
        b.duplicate().get(a);
        return new String(a, StandardCharsets.UTF_8);
    }

    // ---- scenarios ----
    /** Every API the broker lists (ApiVersions v0, as the recording of tests/data/api-versions.json was taken). */
    static void allApis() throws IOException {
        for (var e : readApis().entrySet()) say("all-apis", e.getKey() + " " + e.getValue()[0] + ".." + e.getValue()[1]);
    }

    static Map<Integer, short[]> readApis() throws IOException {
        Map<Integer, short[]> out = new TreeMap<>();
        var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, (short) 0, new ApiVersionsRequestData());
        r.apiKeys().forEach(k -> out.put((int) k.apiKey(), new short[] {k.minVersion(), k.maxVersion()}));
        return out;
    }

    static String apisFrom;

    static final Compression[] LEGACY_CODECS = {Compression.NONE, Compression.gzip().build(), Compression.snappy().build(), Compression.lz4().build()};

    /** Produce v0 to v2 with message sets of format v0 and v1 (each codec), and of v2: stored as v2 batches; read back by Fetch v4. */
    static void produceOld() throws IOException {
        String L = "produce-old";
        for (String kind : new String[] {"create", "append"}) {
            String tp = t("po-" + kind);
            topic(tp, 1, kind.equals("append") ? Map.of("message.timestamp.type", "LogAppendTime") : Map.of());
            for (short v = 0; v <= 2; v++) {
                for (byte magic = 0; magic <= 1; magic++) {
                    for (Compression c : LEGACY_CODECS) {
                        say(L, kind + " v" + v + " m" + magic + " " + c.type().name + " " + produce(v, tp, records(magic, c, 3)));
                    }
                }
                say(L, kind + " v" + v + " m2 none " + produce(v, tp, records((byte) 2, Compression.NONE, 2)));
                say(L, kind + " v" + v + " m2 gzip " + produce(v, tp, records((byte) 2, Compression.gzip().build(), 2)));
                // Two message sets of mixed formats in one request (the broker does not hold v0 to v2 to one batch).
                say(L, kind + " v" + v + " m0+m1 " + produce(v, tp, concat(records((byte) 0, Compression.NONE, 1), records((byte) 1, Compression.NONE, 2))));
                say(L, kind + " v" + v + " m1 gzip+m1 none " + produce(v, tp, concat(records((byte) 1, Compression.gzip().build(), 2), records((byte) 1, Compression.NONE, 1))));
                say(L, kind + " v" + v + " m1+m2 " + produce(v, tp, concat(records((byte) 1, Compression.NONE, 1), records((byte) 2, Compression.NONE, 1))));
                say(L, kind + " v" + v + " empty " + produce(v, tp, ByteBuffer.allocate(0)));
            }
            if (max(ApiKeys.PRODUCE) >= 3) say(L, kind + " v3 m1 " + guarded(() -> produce((short) 3, tp, records((byte) 1, Compression.NONE, 1))));
            // Every batch, one per line.
            String line = fetch((short) 4, tp, 0, 1 << 20);
            for (String part : line.split(" \\[")) say(L, kind + " fetch v4 " + (part.startsWith("m") ? "[" : "") + part);
        }
    }

    /** Message sets the broker refuses: a bad CRC, an inner message set that is compressed again, a magic that does not match. */
    static void produceOldRefused() throws IOException {
        String L = "produce-old-refused", tp = t("pr");
        topic(tp, 1);
        for (short v = 0; v <= 2; v++) {
            ByteBuffer bad = records((byte) 1, Compression.NONE, 1);
            bad.put(30, (byte) (bad.get(30) ^ 0x5a)); // in the value
            say(L, "v" + v + " bad crc m1 " + produce(v, tp, bad));
            ByteBuffer bad0 = records((byte) 0, Compression.NONE, 1);
            bad0.put(bad0.limit() - 1, (byte) (bad0.get(bad0.limit() - 1) ^ 0x5a));
            say(L, "v" + v + " bad crc m0 " + produce(v, tp, bad0));
            ByteBuffer badz = records((byte) 1, Compression.gzip().build(), 2);
            badz.put(badz.limit() - 3, (byte) (badz.get(badz.limit() - 3) ^ 0x5a));
            say(L, "v" + v + " bad crc m1 gzip " + produce(v, tp, badz));
            // A wrapper of format v1 whose attributes say zstd (4).
            ByteBuffer z = records((byte) 1, Compression.gzip().build(), 2);
            z.put(17, (byte) ((z.get(17) & ~7) | 4));
            fixLegacyCrc(z, 0);
            say(L, "v" + v + " m1 zstd wrapper " + produce(v, tp, z));
            // A truncated message set: the last message is cut short.
            ByteBuffer cut = records((byte) 1, Compression.NONE, 2);
            cut.limit(cut.limit() - 3);
            say(L, "v" + v + " truncated " + produce(v, tp, cut));
            // A message with a key on a compacted topic is fine; one without is refused (below).
        }
        String compact = t("prc");
        topic(compact, 1, Map.of("cleanup.policy", "compact"));
        ByteBuffer nokey = ByteBuffer.allocate(1024);
        MemoryRecordsBuilder b = MemoryRecords.builder(nokey, (byte) 1, Compression.NONE, TimestampType.CREATE_TIME, 0L);
        b.append(BASE, null, "v".getBytes());
        for (short v = 0; v <= 2; v++) say(L, "v" + v + " compact no key " + produce(v, compact, b.build().buffer()));
        // A timestamp of format v1 far in the future (message.timestamp.difference.max.ms or .after.max.ms).
        for (short v = 0; v <= 2; v++) say(L, "v" + v + " future " + produce(v, tp, records((byte) 1, Compression.NONE, 1, System.currentTimeMillis() + 3 * 86_400_000L, TimestampType.CREATE_TIME)));
    }

    static void fixLegacyCrc(ByteBuffer b, int at) {
        // offset(8) size(4) crc(4) magic...
        int size = b.getInt(at + 8);
        java.util.zip.CRC32 crc = new java.util.zip.CRC32();
        crc.update(b.array(), b.arrayOffset() + at + 16, size - 4);
        b.putInt(at + 12, (int) crc.getValue());
    }

    /** What Fetch v0 to v3 read of v2 batches: down-converted to format v0 (v0, v1) or v1 (v2, v3), from each offset. */
    static void fetchOld() throws IOException {
        String L = "fetch-old";
        for (String kind : new String[] {"create", "append"}) {
            String tp = t("fo-" + kind);
            topic(tp, 1, kind.equals("append") ? Map.of("message.timestamp.type", "LogAppendTime") : Map.of());
            short pv = max(ApiKeys.PRODUCE);
            produce(pv, tp, records((byte) 2, Compression.NONE, 3));
            produce(pv, tp, records((byte) 2, Compression.gzip().build(), 3));
            produce(pv, tp, records((byte) 2, Compression.snappy().build(), 2));
            produce(pv, tp, records((byte) 2, Compression.lz4().build(), 2));
            for (short v = 0; v <= 4; v++) {
                for (long off : new long[] {0, 1, 4, 6, 9, 10, 11}) say(L, kind + " v" + v + " @" + off + " " + fetch(v, tp, off, 1 << 20));
            }
            // A partition limit smaller than what is converted, and smaller than one batch.
            for (short v = 0; v <= 3; v++) {
                for (int maxBytes : new int[] {40, 100, 150, 250}) say(L, kind + " v" + v + " @0 max " + maxBytes + " " + fetch(v, tp, 0, maxBytes));
            }
        }
    }

    /** Transactions read by Fetch v0 to v3: a committed and an aborted one, with their markers; and idempotent batches. */
    static void fetchOldTxn() throws IOException {
        String L = "fetch-old-txn", tp = t("ft"), tx = tp + "-tx";
        topic(tp, 1);
        coordinator((byte) 1, tx);
        var init = (InitProducerIdResponseData) retry(() -> ask(ApiKeys.INIT_PRODUCER_ID, (short) 0, new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(60000)), r -> ((InitProducerIdResponseData) r).errorCode());
        long pid = init.producerId();
        short epoch = init.producerEpoch();
        int seq = 0;
        for (boolean commit : new boolean[] {true, false}) {
            var add = new AddPartitionsToTxnRequestData().setV3AndBelowTransactionalId(tx).setV3AndBelowProducerId(pid).setV3AndBelowProducerEpoch(epoch);
            var at = new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopic().setName(tp).setPartitions(List.of(0));
            add.v3AndBelowTopics().add(at);
            retry(() -> ask(ApiKeys.ADD_PARTITIONS_TO_TXN, (short) 0, add), r -> ((AddPartitionsToTxnResponseData) r).resultsByTopicV3AndBelow().iterator().next().resultsByPartition().iterator().next().partitionErrorCode());
            ByteBuffer recs = MemoryRecords.withTransactionalRecords(Compression.NONE, pid, epoch, seq, new SimpleRecord(BASE, "k".getBytes(), (commit ? "committed" : "aborted").getBytes()), new SimpleRecord((BASE + 1), "k".getBytes(), "two".getBytes())).buffer();
            seq += 2;
            say(L, (commit ? "commit" : "abort") + " produce " + produce((short) 3, tp, 0, recs, tx));
            var end = (EndTxnResponseData) retry(() -> ask(ApiKeys.END_TXN, (short) 0, new EndTxnRequestData().setTransactionalId(tx).setProducerId(pid).setProducerEpoch(epoch).setCommitted(commit)), r -> ((EndTxnResponseData) r).errorCode());
            say(L, (commit ? "commit" : "abort") + " end " + end.errorCode());
            if (commit) {
                // Only the marker past the records: nothing to convert.
                sleep(300);
                for (short v = 0; v <= 3; v++) say(L, "only the marker v" + v + " @2 " + fetch(v, tp, 2, 1 << 20));
            }
        }
        sleep(500);
        var idem = (InitProducerIdResponseData) ask(ApiKeys.INIT_PRODUCER_ID, (short) 0, new InitProducerIdRequestData().setTransactionalId(null).setTransactionTimeoutMs(60000));
        say(L, "idempotent " + produce((short) 3, tp, 0, MemoryRecords.withIdempotentRecords(Compression.NONE, idem.producerId(), idem.producerEpoch(), 0, new SimpleRecord(BASE, "i".getBytes(), "idem".getBytes())).buffer(), null));
        for (short v = 0; v <= 4; v++) for (long off : new long[] {0, 2, 3, 5, 6}) say(L, "v" + v + " @" + off + " " + fetch(v, tp, off, 1 << 20));
    }

    interface Asker { ApiMessage ask() throws IOException; }
    interface Code { short of(ApiMessage m); }

    static ApiMessage retry(Asker a, Code c) throws IOException {
        for (int i = 0; ; i++) {
            ApiMessage m = a.ask();
            short code = c.of(m);
            if (List.of(14, 15, 16, 51, 7).contains((int) code) && i < 100) { sleep(150); continue; }
            return m;
        }
    }

    /** A zstd topic read by Fetch below v10, and v0 to v2 of Produce with zstd. */
    static void zstd() throws IOException {
        String L = "zstd", tp = t("z");
        topic(tp, 1, Map.of("compression.type", "zstd"));
        short pv = max(ApiKeys.PRODUCE);
        if (major >= 2) say(L, "produce v" + pv + " " + produce(pv, tp, records((byte) 2, Compression.NONE, 2)));
        for (short v = 0; v <= 2; v++) say(L, "produce v" + v + " m1 " + produce(v, tp, records((byte) 1, Compression.NONE, 1)));
        for (short v : new short[] {0, 2, 3, 4, 9, 10}) if (v <= max(ApiKeys.FETCH)) say(L, "fetch v" + v + " " + fetch(v, tp, 0, 1 << 20));
    }

    /** ListOffsets v0 (a list of offsets, max_num_offsets) and every later version the broker lists, on a topic with records. */
    static void listOffsets() throws IOException {
        String L = "list-offsets", tp = t("lo"), empty = t("loe");
        topic(tp, 1);
        topic(empty, 1);
        short pv = max(ApiKeys.PRODUCE);
        produce(pv, tp, records((byte) 2, Compression.NONE, 3, BASE, TimestampType.CREATE_TIME));
        produce(pv, tp, records((byte) 2, Compression.NONE, 2, (BASE + 100_000), TimestampType.CREATE_TIME));
        for (short v = 0; v <= max(ApiKeys.LIST_OFFSETS); v++) {
            for (long ts : new long[] {-1, -2, -3, 0, (BASE + 1), (BASE + 100_000), BASE + 10_000_000}) {
                if (ts == -3 && v < 7) continue;
                for (int n : v == 0 ? new int[] {1, 2, 10, 0} : new int[] {1}) {
                    for (String topic : new String[] {tp, empty}) {
                        var d = new ListOffsetsRequestData().setReplicaId(-1).setIsolationLevel((byte) 0);
                        var lt = new ListOffsetsRequestData.ListOffsetsTopic().setName(topic);
                        lt.partitions().add(new ListOffsetsRequestData.ListOffsetsPartition().setPartitionIndex(0).setTimestamp(ts).setMaxNumOffsets(n).setCurrentLeaderEpoch(-1));
                        d.topics().add(lt);
                        var r = (ListOffsetsResponseData) ask(ApiKeys.LIST_OFFSETS, v, d);
                        var p = r.topics().get(0).partitions().get(0);
                        String s = "err=" + p.errorCode();
                        if (v == 0) s += " offsets=" + p.oldStyleOffsets();
                        else s += " offset=" + p.offset() + " time=" + ts(p.timestamp());
                        if (v >= 4) s += " epoch=" + p.leaderEpoch();
                        say(L, "v" + v + " " + (topic.equals(tp) ? "full" : "empty") + " ts=" + ts(ts) + (v == 0 ? " n=" + n : "") + " " + s);
                    }
                }
            }
        }
        // v0 for a partition that does not exist and a topic that does not.
        for (short v = 0; v <= 1; v++) {
            for (String[] target : new String[][] {{tp, "3"}, {t("nothing"), "0"}}) {
                var d = new ListOffsetsRequestData().setReplicaId(-1);
                var lt = new ListOffsetsRequestData.ListOffsetsTopic().setName(target[0]);
                lt.partitions().add(new ListOffsetsRequestData.ListOffsetsPartition().setPartitionIndex(Integer.parseInt(target[1])).setTimestamp(-1).setMaxNumOffsets(1));
                d.topics().add(lt);
                say(L, "v" + v + " " + target[0].replace(run, "R") + "-" + target[1] + " " + norm(ask(ApiKeys.LIST_OFFSETS, v, d)));
            }
        }
    }

    /** OffsetCommit v0 (ZooKeeper in Kafka 1 and 2) and v1 (a commit timestamp), read back by OffsetFetch v0 and v1 and later. */
    static void offsetsOld() throws IOException {
        String L = "offsets-old", tp = t("oc");
        topic(tp, 2);
        String g = t("g");
        coordinator((byte) 0, g);
        for (short v = 0; v <= max(ApiKeys.OFFSET_COMMIT); v++) {
            var d = new OffsetCommitRequestData().setGroupId(g).setGenerationIdOrMemberEpoch(-1).setMemberId("").setRetentionTimeMs(-1);
            var ot = new OffsetCommitRequestData.OffsetCommitRequestTopic().setName(tp);
            ot.partitions().add(new OffsetCommitRequestData.OffsetCommitRequestPartition().setPartitionIndex(v % 2).setCommittedOffset(100 + v).setCommitTimestamp(v == 1 ? BASE : -1).setCommittedMetadata("m" + v).setCommittedLeaderEpoch(-1));
            d.topics().add(ot);
            final short cv = v;
            say(L, "commit v" + v + " " + norm(retry(() -> ask(ApiKeys.OFFSET_COMMIT, cv, d), r -> ((OffsetCommitResponseData) r).topics().get(0).partitions().get(0).errorCode())));
            for (short f = 0; f <= Math.min(max(ApiKeys.OFFSET_FETCH), 7); f++) {
                say(L, "  fetch v" + f + " " + offsetFetchLine(f, g, tp, List.of(0, 1)));
            }
            if (max(ApiKeys.OFFSET_FETCH) >= 2) say(L, "  fetch v2 all " + offsetFetchLine((short) 2, g, null, null));
        }
        // A v0 commit to a group that has members; and an unknown group read at v0.
        say(L, "fetch v0 unknown group " + offsetFetchLine((short) 0, t("nobody"), tp, List.of(0)));
        say(L, "fetch v1 unknown group " + offsetFetchLine((short) 1, t("nobody"), tp, List.of(0)));
        // DescribeGroups and ListGroups see the group v0 committed to?
        String g0 = t("g0");
        coordinator((byte) 0, g0);
        var d = new OffsetCommitRequestData().setGroupId(g0).setGenerationIdOrMemberEpoch(-1).setMemberId("");
        var ot = new OffsetCommitRequestData.OffsetCommitRequestTopic().setName(tp);
        ot.partitions().add(new OffsetCommitRequestData.OffsetCommitRequestPartition().setPartitionIndex(0).setCommittedOffset(5).setCommittedMetadata(""));
        d.topics().add(ot);
        say(L, "v0 only commit " + norm(ask(ApiKeys.OFFSET_COMMIT, (short) 0, d)));
        say(L, "v0 only fetch v0 " + offsetFetchLine((short) 0, g0, tp, List.of(0)));
        say(L, "v0 only fetch v1 " + offsetFetchLine((short) 1, g0, tp, List.of(0)));
        var dg = new DescribeGroupsRequestData().setGroups(List.of(g0));
        say(L, "v0 only describe " + norm(ask(ApiKeys.DESCRIBE_GROUPS, (short) 0, dg)));
        var lg = (ListGroupsResponseData) ask(ApiKeys.LIST_GROUPS, (short) 0, new ListGroupsRequestData());
        say(L, "v0 only listed " + lg.groups().stream().anyMatch(x -> x.groupId().equals(g0)));
        // A v0 commit with a generation (no such member).
        var dm = new OffsetCommitRequestData().setGroupId(g0).setGenerationIdOrMemberEpoch(3).setMemberId("m");
        dm.topics().add(new OffsetCommitRequestData.OffsetCommitRequestTopic().setName(tp).setPartitions(List.of(new OffsetCommitRequestData.OffsetCommitRequestPartition().setPartitionIndex(0).setCommittedOffset(6))));
        say(L, "v0 member commit " + norm(ask(ApiKeys.OFFSET_COMMIT, (short) 0, dm)));
        say(L, "v1 member commit " + norm(ask(ApiKeys.OFFSET_COMMIT, (short) 1, dm)));
        // An unknown topic at v0.
        var du = new OffsetCommitRequestData().setGroupId(g0).setGenerationIdOrMemberEpoch(-1).setMemberId("");
        du.topics().add(new OffsetCommitRequestData.OffsetCommitRequestTopic().setName(t("nothing")).setPartitions(List.of(new OffsetCommitRequestData.OffsetCommitRequestPartition().setPartitionIndex(0).setCommittedOffset(6))));
        say(L, "v0 unknown topic " + norm(ask(ApiKeys.OFFSET_COMMIT, (short) 0, du)));
        say(L, "v1 unknown topic " + norm(ask(ApiKeys.OFFSET_COMMIT, (short) 1, du)));
    }


    static String offsetFetchLine(short v, String g, String tp, List<Integer> parts) throws IOException {
        var d = new OffsetFetchRequestData().setGroupId(g);
        if (tp != null) d.topics().add(new OffsetFetchRequestData.OffsetFetchRequestTopic().setName(tp).setPartitionIndexes(parts));
        else d.setTopics(null);
        var r = (OffsetFetchResponseData) ask(ApiKeys.OFFSET_FETCH, v, d);
        List<String> each = new ArrayList<>();
        for (var t : r.topics()) for (var p : t.partitions()) each.add(t.name().replace(run, "R") + "-" + p.partitionIndex() + "=" + p.errorCode() + "@" + p.committedOffset() + "'" + p.metadata() + "'" + (v >= 5 ? "e" + p.committedLeaderEpoch() : ""));
        Collections.sort(each);
        return "err=" + r.errorCode() + " " + String.join(" ", each);
    }

    /** CreateTopics v0 and v1 (validate only), DeleteTopics v0, DescribeConfigs v0 (is_default, not the source). */
    static void adminOld() throws IOException {
        String L = "admin-old";
        for (short v = 0; v <= max(ApiKeys.CREATE_TOPICS); v++) {
            String name = t("ct" + v);
            var d = new CreateTopicsRequestData().setTimeoutMs(10000).setValidateOnly(v >= 1 && v % 2 == 1);
            d.topics().add(new CreateTopicsRequestData.CreatableTopic().setName(name).setNumPartitions(2).setReplicationFactor((short) 1)
                .setConfigs(new CreateTopicsRequestData.CreatableTopicConfigCollection(List.of(new CreateTopicsRequestData.CreatableTopicConfig().setName("retention.ms").setValue("1000000")).iterator())));
            d.topics().add(new CreateTopicsRequestData.CreatableTopic().setName(t("bad" + v)).setNumPartitions(-5).setReplicationFactor((short) 1));
            d.topics().add(new CreateTopicsRequestData.CreatableTopic().setName(t("cfg" + v)).setNumPartitions(1).setReplicationFactor((short) 1)
                .setConfigs(new CreateTopicsRequestData.CreatableTopicConfigCollection(List.of(new CreateTopicsRequestData.CreatableTopicConfig().setName("no.such.config").setValue("1")).iterator())));
            say(L, "create v" + v + " " + norm(ask(ApiKeys.CREATE_TOPICS, v, d)));
            if (v == 0) say(L, "create v0 again " + norm(ask(ApiKeys.CREATE_TOPICS, v, d)));
        }
        sleep(500);
        for (short v = 0; v <= max(ApiKeys.DELETE_TOPICS); v++) {
            var d = new DeleteTopicsRequestData().setTimeoutMs(10000).setTopicNames(List.of(t("ct" + (v * 2)), t("nothing")));
            if (v >= 6) {
                d.setTopicNames(List.of());
                d.setTopics(List.of(new DeleteTopicsRequestData.DeleteTopicState().setName(t("ct" + (v * 2))), new DeleteTopicsRequestData.DeleteTopicState().setName(t("nothing"))));
            }
            say(L, "delete v" + v + " " + norm(ask(ApiKeys.DELETE_TOPICS, v, d)));
        }
        String tp = t("dc");
        topic(tp, 1, Map.of("retention.ms", "1234567", "cleanup.policy", "compact"));
        for (short v = 0; v <= max(ApiKeys.DESCRIBE_CONFIGS); v++) {
            for (boolean synonyms : new boolean[] {false, true}) {
                if (synonyms && v < 1) continue;
                var d = new DescribeConfigsRequestData().setIncludeSynonyms(synonyms).setIncludeDocumentation(v >= 3);
                d.resources().add(new DescribeConfigsRequestData.DescribeConfigsResource().setResourceType((byte) 2).setResourceName(tp).setConfigurationKeys(List.of("retention.ms", "cleanup.policy", "segment.bytes", "min.insync.replicas")));
                d.resources().add(new DescribeConfigsRequestData.DescribeConfigsResource().setResourceType((byte) 2).setResourceName(t("nothing")).setConfigurationKeys(null));
                var r = (DescribeConfigsResponseData) ask(ApiKeys.DESCRIBE_CONFIGS, v, d);
                for (var res : r.results()) {
                    StringBuilder s = new StringBuilder("err=" + res.errorCode() + " " + res.resourceName().replace(run, "R"));
                    for (var c : res.configs()) {
                        s.append(" ").append(c.name()).append("=").append(c.value()).append(" ro=").append(c.readOnly());
                        if (v == 0) s.append(" default=").append(c.configSource() == 5 || c.isDefault());
                        else s.append(" src=").append(c.configSource());
                        s.append(" sens=").append(c.isSensitive());
                        if (synonyms) s.append(" syn=").append(c.synonyms().size());
                        if (v >= 3) s.append(" type=").append(c.configType()).append(" doc=").append(c.documentation() != null);
                    }
                    say(L, "describe v" + v + (synonyms ? " synonyms" : "") + " " + s);
                }
            }
        }
        // Every config of a topic at v0, its count, and which are not default.
        var d = new DescribeConfigsRequestData();
        d.resources().add(new DescribeConfigsRequestData.DescribeConfigsResource().setResourceType((byte) 2).setResourceName(tp).setConfigurationKeys(null));
        var r = (DescribeConfigsResponseData) ask(ApiKeys.DESCRIBE_CONFIGS, (short) 0, d);
        List<String> names = new ArrayList<>();
        for (var c : r.results().get(0).configs()) names.add(c.name() + (c.isDefault() ? "" : "*") + (c.readOnly() ? "(ro)" : ""));
        Collections.sort(names);
        say(L, "describe v0 all " + names.size() + " " + names);
        // The broker's own configs at v0: how many, and that each is read only or not.
        var db = new DescribeConfigsRequestData();
        db.resources().add(new DescribeConfigsRequestData.DescribeConfigsResource().setResourceType((byte) 4).setResourceName("1").setConfigurationKeys(List.of("log.retention.hours", "num.partitions", "message.max.bytes")));
        var rb = (DescribeConfigsResponseData) ask(ApiKeys.DESCRIBE_CONFIGS, (short) 0, db);
        for (var c : rb.results().get(0).configs()) say(L, "describe v0 broker " + c.name() + "=" + c.value() + " ro=" + c.readOnly() + " default=" + c.isDefault() + " sens=" + c.isSensitive());
    }

    /** Metadata at every version the broker lists: a known topic, an unknown one, all topics (v0 has no null list: empty is all). */
    static void metadata() throws IOException {
        String L = "metadata", tp = t("md");
        topic(tp, 2);
        for (short v = 0; v <= max(ApiKeys.METADATA); v++) {
            var d = new MetadataRequestData().setAllowAutoTopicCreation(v < 4);
            d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(tp));
            d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(t("absent-md" + v)));
            if (v >= 8 && v <= 10) d.setIncludeClusterAuthorizedOperations(true).setIncludeTopicAuthorizedOperations(true);
            var r = (MetadataResponseData) ask(ApiKeys.METADATA, v, d);
            List<String> topics = new ArrayList<>();
            for (var t : r.topics()) {
                String s = t.name().replace(run, "R") + "=" + t.errorCode() + " internal=" + t.isInternal() + " parts=" + t.partitions().size();
                if (v >= 8) s += " ops=" + t.topicAuthorizedOperations();
                if (v >= 10) s += " id=" + (t.topicId().equals(Uuid.ZERO_UUID) ? "NIL" : "ID");
                for (var p : t.partitions()) s += " [" + p.partitionIndex() + " e=" + p.errorCode() + " l=" + p.leaderId() + " ep=" + p.leaderEpoch() + " r=" + p.replicaNodes() + " isr=" + p.isrNodes() + " off=" + p.offlineReplicas() + "]";
                topics.add(s);
            }
            Collections.sort(topics);
            String s = "brokers=" + r.brokers().size() + " rack=" + r.brokers().iterator().next().rack() + " controller=" + r.controllerId() + " cluster=" + (r.clusterId() == null ? "null" : "C");
            if (v >= 8 && v <= 10) s += " clusterOps=" + r.clusterAuthorizedOperations();
            say(L, "v" + v + " " + s + " " + topics);
        }
        // v0's empty list is every topic; from v1 null is every topic and empty is none.
        for (short v = 0; v <= 1; v++) {
            var d = new MetadataRequestData().setTopics(v == 0 ? List.of() : null);
            var r = (MetadataResponseData) ask(ApiKeys.METADATA, v, d);
            say(L, "v" + v + " all has topic " + r.topics().stream().anyMatch(x -> x.name().equals(tp)) + " internal listed " + r.topics().stream().anyMatch(x -> x.isInternal()));
        }
        if (max(ApiKeys.METADATA) >= 1) {
            var r = (MetadataResponseData) ask(ApiKeys.METADATA, (short) 1, new MetadataRequestData().setTopics(List.of()));
            say(L, "v1 empty list " + r.topics().size());
        }
        // Auto-creation at v0 to v3 (always asked) and v4 (the flag).
        for (short v = 0; v <= Math.min(4, max(ApiKeys.METADATA)); v++) {
            var d = new MetadataRequestData().setAllowAutoTopicCreation(true);
            d.topics().add(new MetadataRequestData.MetadataRequestTopic().setName(t("auto" + v)));
            var r = (MetadataResponseData) ask(ApiKeys.METADATA, v, d);
            say(L, "v" + v + " auto " + r.topics().iterator().next().errorCode());
        }
    }

    /** The classic group APIs at every version the broker lists: a member joins, syncs, heartbeats, is described, leaves. */
    static void groups() throws IOException {
        String L = "groups", tp = t("gr");
        topic(tp, 1);
        for (short jv = 0; jv <= max(ApiKeys.JOIN_GROUP); jv++) {
            String g = t("gj" + jv);
            coordinator((byte) 0, g);
            var join = new JoinGroupRequestData().setGroupId(g).setSessionTimeoutMs(10000).setRebalanceTimeoutMs(10000).setMemberId("").setProtocolType("consumer");
            join.protocols().add(new JoinGroupRequestData.JoinGroupRequestProtocol().setName("range").setMetadata(new byte[] {0, 0, 0, 0, 0, 0, 0, 0}));
            final short v = jv;
            var r = (JoinGroupResponseData) ask(ApiKeys.JOIN_GROUP, v, join);
            say(L, "join v" + v + " " + norm(r));
            if (r.errorCode() == 79) { // MEMBER_ID_REQUIRED (v4 on)
                join.setMemberId(r.memberId());
                r = (JoinGroupResponseData) ask(ApiKeys.JOIN_GROUP, v, join);
                say(L, "join v" + v + " again " + norm(r));
            }
            String member = r.memberId();
            int gen = r.generationId();
            for (short s = 0; s <= max(ApiKeys.SYNC_GROUP); s++) {
                var sync = new SyncGroupRequestData().setGroupId(g).setGenerationId(gen).setMemberId(member).setProtocolType("consumer").setProtocolName("range");
                sync.assignments().add(new SyncGroupRequestData.SyncGroupRequestAssignment().setMemberId(member).setAssignment(new byte[] {0, 1}));
                say(L, "  sync v" + s + " " + norm(ask(ApiKeys.SYNC_GROUP, s, sync)));
            }
            for (short h = 0; h <= max(ApiKeys.HEARTBEAT); h++) {
                say(L, "  heartbeat v" + h + " " + norm(ask(ApiKeys.HEARTBEAT, h, new HeartbeatRequestData().setGroupId(g).setGenerationId(gen).setMemberId(member))));
                say(L, "  heartbeat v" + h + " bad gen " + norm(ask(ApiKeys.HEARTBEAT, h, new HeartbeatRequestData().setGroupId(g).setGenerationId(gen + 5).setMemberId(member))));
            }
            for (short dv = 0; dv <= max(ApiKeys.DESCRIBE_GROUPS); dv++) {
                var dg = new DescribeGroupsRequestData().setGroups(List.of(g, t("nothing"))).setIncludeAuthorizedOperations(dv >= 3);
                say(L, "  describe v" + dv + " " + norm(ask(ApiKeys.DESCRIBE_GROUPS, dv, dg)));
            }
            for (short lv = 0; lv <= max(ApiKeys.LIST_GROUPS); lv++) {
                var lg = (ListGroupsResponseData) ask(ApiKeys.LIST_GROUPS, lv, new ListGroupsRequestData());
                say(L, "  list v" + lv + " " + lg.groups().stream().filter(x -> x.groupId().equals(g)).map(Probe::norm).toList());
                if (lv >= 4) {
                    var lgs = (ListGroupsResponseData) ask(ApiKeys.LIST_GROUPS, lv, new ListGroupsRequestData().setStatesFilter(List.of("Stable")));
                    say(L, "  list v" + lv + " stable " + lgs.groups().stream().filter(x -> x.groupId().equals(g)).map(Probe::norm).toList());
                }
            }
            short lv = (short) Math.min(max(ApiKeys.LEAVE_GROUP), jv);
            var leave = new LeaveGroupRequestData().setGroupId(g).setMemberId(member);
            if (lv >= 3) leave.setMemberId("").setMembers(List.of(new LeaveGroupRequestData.MemberIdentity().setMemberId(member), new LeaveGroupRequestData.MemberIdentity().setMemberId("nobody")));
            say(L, "  leave v" + lv + " " + norm(ask(ApiKeys.LEAVE_GROUP, lv, leave)));
        }
        // Static membership (JoinGroup v5 on).
        if (max(ApiKeys.JOIN_GROUP) >= 5) {
            String g = t("gs");
            coordinator((byte) 0, g);
            var join = new JoinGroupRequestData().setGroupId(g).setSessionTimeoutMs(10000).setRebalanceTimeoutMs(10000).setMemberId("").setGroupInstanceId("i1").setProtocolType("consumer");
            join.protocols().add(new JoinGroupRequestData.JoinGroupRequestProtocol().setName("range").setMetadata(new byte[] {0}));
            short v = max(ApiKeys.JOIN_GROUP);
            say(L, "static join v" + v + " " + norm(ask(ApiKeys.JOIN_GROUP, v, join)));
            say(L, "static join v" + v + " again " + norm(ask(ApiKeys.JOIN_GROUP, v, join)));
        }
        for (short v = 0; v <= max(ApiKeys.DELETE_GROUPS); v++) {
            say(L, "delete groups v" + v + " " + norm(ask(ApiKeys.DELETE_GROUPS, v, new DeleteGroupsRequestData().setGroupsNames(List.of(t("gj0"), t("nothing"))))));
        }
    }

    /** FindCoordinator at every version the broker lists, for a group and a transactional id (and a share key at v6). */
    static void findCoordinator() throws IOException {
        String L = "find-coordinator";
        for (short v = 0; v <= max(ApiKeys.FIND_COORDINATOR); v++) {
            for (byte type : new byte[] {0, 1, 2}) {
                if (type == 1 && v == 0) continue;
                if (type == 2 && v < 6) continue;
                var d = new FindCoordinatorRequestData().setKeyType(type);
                String key = type == 2 ? t("share") + ":" + "AAAAAAAAAAAAAAAAAAAAAA" + ":0" : t("fc" + type);
                if (v >= 4) d.setCoordinatorKeys(List.of(key, "")); else d.setKey(key);
                final short fv = v;
                var r = retry(() -> ask(ApiKeys.FIND_COORDINATOR, fv, d), x -> fv >= 4 ? ((FindCoordinatorResponseData) x).coordinators().get(0).errorCode() : ((FindCoordinatorResponseData) x).errorCode());
                say(L, "v" + v + " type " + type + " " + norm(r).replaceAll("host='[^']*'", "host=H").replaceAll("port=[0-9]+", "port=N"));
            }
        }
    }

    /** The transactional APIs at every version the broker lists: init, add a partition and the offsets, commit offsets, end. */
    static void txn() throws IOException {
        String L = "txn", tp = t("tx");
        topic(tp, 1);
        for (short v = 0; v <= max(ApiKeys.INIT_PRODUCER_ID); v++) {
            String tx = t("txid" + v);
            coordinator((byte) 1, tx);
            var d = new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(60000).setProducerId(-1).setProducerEpoch((short) -1);
            final short iv = v;
            var r = (InitProducerIdResponseData) retry(() -> ask(ApiKeys.INIT_PRODUCER_ID, iv, d), x -> ((InitProducerIdResponseData) x).errorCode());
            say(L, "init v" + v + " " + norm(r));
            long pid = r.producerId();
            short ep = r.producerEpoch();
            if (v >= 3) {
                var again = new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(60000).setProducerId(pid).setProducerEpoch(ep);
                var r2 = (InitProducerIdResponseData) retry(() -> ask(ApiKeys.INIT_PRODUCER_ID, iv, again), x -> ((InitProducerIdResponseData) x).errorCode());
                say(L, "init v" + v + " bump " + norm(r2) + " same=" + (r2.producerId() == pid) + " epoch+1=" + (r2.producerEpoch() == ep + 1));
                ep = r2.producerEpoch();
            }
            short av = (short) Math.min(v, max(ApiKeys.ADD_PARTITIONS_TO_TXN));
            av = (short) Math.min(av, 3);
            var add = new AddPartitionsToTxnRequestData().setV3AndBelowTransactionalId(tx).setV3AndBelowProducerId(pid).setV3AndBelowProducerEpoch(ep);
            add.v3AndBelowTopics().add(new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopic().setName(tp).setPartitions(List.of(0)));
            final short fav = av;
            say(L, "  add partitions v" + av + " " + norm(retry(() -> ask(ApiKeys.ADD_PARTITIONS_TO_TXN, fav, add), x -> ((AddPartitionsToTxnResponseData) x).resultsByTopicV3AndBelow().iterator().next().resultsByPartition().iterator().next().partitionErrorCode())));
            String g = t("txg" + v);
            coordinator((byte) 0, g);
            short ov = (short) Math.min(v, max(ApiKeys.ADD_OFFSETS_TO_TXN));
            final short fov = ov;
            final long fpid = pid;
            final short fep = ep;
            say(L, "  add offsets v" + ov + " " + norm(retry(() -> ask(ApiKeys.ADD_OFFSETS_TO_TXN, fov, new AddOffsetsToTxnRequestData().setTransactionalId(tx).setProducerId(fpid).setProducerEpoch(fep).setGroupId(g)), x -> ((AddOffsetsToTxnResponseData) x).errorCode())));
            short cv = (short) Math.min(v, max(ApiKeys.TXN_OFFSET_COMMIT));
            var toc = new TxnOffsetCommitRequestData().setTransactionalId(tx).setGroupId(g).setProducerId(pid).setProducerEpoch(ep).setMemberId("").setGenerationId(-1);
            toc.topics().add(new TxnOffsetCommitRequestData.TxnOffsetCommitRequestTopic().setName(tp).setPartitions(List.of(new TxnOffsetCommitRequestData.TxnOffsetCommitRequestPartition().setPartitionIndex(0).setCommittedOffset(3).setCommittedLeaderEpoch(-1).setCommittedMetadata("x"))));
            final short fcv = cv;
            say(L, "  txn offset commit v" + cv + " " + norm(retry(() -> ask(ApiKeys.TXN_OFFSET_COMMIT, fcv, toc), x -> ((TxnOffsetCommitResponseData) x).topics().get(0).partitions().get(0).errorCode())));
            short ev = (short) Math.min(v, max(ApiKeys.END_TXN));
            final short fev = ev;
            say(L, "  end v" + ev + " " + norm(retry(() -> ask(ApiKeys.END_TXN, fev, new EndTxnRequestData().setTransactionalId(tx).setProducerId(fpid).setProducerEpoch(fep).setCommitted(true)), x -> ((EndTxnResponseData) x).errorCode())));
            sleep(300);
            say(L, "  committed " + offsetFetchLine((short) Math.min(max(ApiKeys.OFFSET_FETCH), 7), g, tp, List.of(0)));
            if (has(ApiKeys.DESCRIBE_TRANSACTIONS)) say(L, "  described " + norm(ask(ApiKeys.DESCRIBE_TRANSACTIONS, (short) 0, new DescribeTransactionsRequestData().setTransactionalIds(List.of(tx)))).replaceAll("transactionStartTimeMs=-?[0-9]+", "start=_"));
        }
        // AddPartitionsToTxn / AddOffsetsToTxn / EndTxn at every version with a fenced epoch.
        String tx = t("txf");
        coordinator((byte) 1, tx);
        var r = (InitProducerIdResponseData) retry(() -> ask(ApiKeys.INIT_PRODUCER_ID, (short) 0, new InitProducerIdRequestData().setTransactionalId(tx).setTransactionTimeoutMs(60000)), x -> ((InitProducerIdResponseData) x).errorCode());
        for (short v = 0; v <= Math.min(3, max(ApiKeys.ADD_PARTITIONS_TO_TXN)); v++) {
            var add = new AddPartitionsToTxnRequestData().setV3AndBelowTransactionalId(tx).setV3AndBelowProducerId(r.producerId()).setV3AndBelowProducerEpoch((short) (r.producerEpoch() - 1));
            add.v3AndBelowTopics().add(new AddPartitionsToTxnRequestData.AddPartitionsToTxnTopic().setName(tp).setPartitions(List.of(0)));
            say(L, "fenced add v" + v + " " + norm(ask(ApiKeys.ADD_PARTITIONS_TO_TXN, v, add)));
        }
        for (short v = 0; v <= max(ApiKeys.END_TXN); v++) {
            say(L, "fenced end v" + v + " " + norm(ask(ApiKeys.END_TXN, v, new EndTxnRequestData().setTransactionalId(tx).setProducerId(r.producerId()).setProducerEpoch((short) (r.producerEpoch() - 1)).setCommitted(false))));
        }
        for (short v = 0; v <= max(ApiKeys.END_TXN); v++) {
            say(L, "no txn end v" + v + " " + norm(ask(ApiKeys.END_TXN, v, new EndTxnRequestData().setTransactionalId(tx).setProducerId(r.producerId()).setProducerEpoch(r.producerEpoch()).setCommitted(true))));
        }
    }

    /** DeleteRecords, CreatePartitions, the ACL APIs, DescribeCluster, DescribeProducers at every version the broker lists. */
    static void adminMore() throws IOException {
        String L = "admin-more", tp = t("am");
        topic(tp, 1);
        short pv = max(ApiKeys.PRODUCE);
        produce(pv, tp, records((byte) 2, Compression.NONE, 5));
        for (short v = 0; v <= max(ApiKeys.DELETE_RECORDS); v++) {
            var d = new DeleteRecordsRequestData().setTimeoutMs(5000);
            d.topics().add(new DeleteRecordsRequestData.DeleteRecordsTopic().setName(tp).setPartitions(List.of(new DeleteRecordsRequestData.DeleteRecordsPartition().setPartitionIndex(0).setOffset(1 + v), new DeleteRecordsRequestData.DeleteRecordsPartition().setPartitionIndex(4).setOffset(1))));
            say(L, "delete records v" + v + " " + norm(ask(ApiKeys.DELETE_RECORDS, v, d)));
        }
        for (short v = 0; v <= max(ApiKeys.CREATE_PARTITIONS); v++) {
            var d = new CreatePartitionsRequestData().setTimeoutMs(5000).setValidateOnly(v % 2 == 1);
            d.topics().add(new CreatePartitionsRequestData.CreatePartitionsTopic().setName(tp).setCount(2 + v).setAssignments(null));
            d.topics().add(new CreatePartitionsRequestData.CreatePartitionsTopic().setName(t("nothing")).setCount(2).setAssignments(null));
            say(L, "create partitions v" + v + " " + norm(ask(ApiKeys.CREATE_PARTITIONS, v, d)));
        }
        for (short v = 0; v <= max(ApiKeys.DESCRIBE_ACLS); v++) {
            var d = new DescribeAclsRequestData().setResourceTypeFilter((byte) 1).setPatternTypeFilter((byte) (v == 0 ? 3 : 1)).setOperation((byte) 1).setPermissionType((byte) 1);
            say(L, "describe acls v" + v + " " + norm(ask(ApiKeys.DESCRIBE_ACLS, v, d)));
        }
        for (short v = 0; v <= max(ApiKeys.CREATE_ACLS); v++) {
            var d = new CreateAclsRequestData();
            d.creations().add(new CreateAclsRequestData.AclCreation().setResourceType((byte) 2).setResourceName(tp).setResourcePatternType((byte) 3).setPrincipal("User:a" + v).setHost("*").setOperation((byte) 3).setPermissionType((byte) 3));
            say(L, "create acls v" + v + " " + norm(ask(ApiKeys.CREATE_ACLS, v, d)));
        }
        if (has(ApiKeys.DESCRIBE_CLUSTER)) {
            for (short v = 0; v <= max(ApiKeys.DESCRIBE_CLUSTER); v++) {
                say(L, "describe cluster v" + v + " " + norm(ask(ApiKeys.DESCRIBE_CLUSTER, v, new DescribeClusterRequestData().setIncludeClusterAuthorizedOperations(true))).replaceAll("host='[^']*'", "host=H").replaceAll("port=[0-9]+", "port=N"));
                say(L, "describe cluster v" + v + " no ops " + norm(ask(ApiKeys.DESCRIBE_CLUSTER, v, new DescribeClusterRequestData())).replaceAll("host='[^']*'", "host=H").replaceAll("port=[0-9]+", "port=N"));
            }
        }
        if (has(ApiKeys.DESCRIBE_PRODUCERS)) {
            var idem = (InitProducerIdResponseData) ask(ApiKeys.INIT_PRODUCER_ID, (short) 0, new InitProducerIdRequestData().setTransactionalId(null).setTransactionTimeoutMs(60000));
            produce((short) 3, tp, 0, MemoryRecords.withIdempotentRecords(Compression.NONE, idem.producerId(), idem.producerEpoch(), 0, new SimpleRecord("i".getBytes())).buffer(), null);
            var d = new DescribeProducersRequestData();
            d.topics().add(new DescribeProducersRequestData.TopicRequest().setName(tp).setPartitionIndexes(List.of(0, 9)));
            d.topics().add(new DescribeProducersRequestData.TopicRequest().setName(t("nothing")).setPartitionIndexes(List.of(0)));
            say(L, "describe producers v0 " + norm(ask(ApiKeys.DESCRIBE_PRODUCERS, (short) 0, d)).replaceAll("lastTimestamp=-?[0-9]+", "lastTimestamp=_").replaceAll("currentTxnStartOffset=-?[0-9]+", "currentTxnStartOffset=_"));
        }
    }

    /** ConsumerGroupHeartbeat and ConsumerGroupDescribe where the broker lists them (Kafka 3.9: v0 of each). */
    static void consumerGroups() throws IOException {
        String L = "consumer-groups", tp = t("cg");
        if (!has(ApiKeys.CONSUMER_GROUP_HEARTBEAT)) { say(L, "not listed"); return; }
        topic(tp, 1);
        String g = t("g848");
        coordinator((byte) 0, g);
        var hb = new ConsumerGroupHeartbeatRequestData().setGroupId(g).setMemberId(Uuid.randomUuid().toString()).setMemberEpoch(0).setRebalanceTimeoutMs(5000).setSubscribedTopicNames(List.of(tp)).setTopicPartitions(List.of());
        say(L, "heartbeat v0 " + norm(ask(ApiKeys.CONSUMER_GROUP_HEARTBEAT, (short) 0, hb)));
        if (has(ApiKeys.CONSUMER_GROUP_DESCRIBE)) say(L, "describe v0 " + norm(ask(ApiKeys.CONSUMER_GROUP_DESCRIBE, (short) 0, new ConsumerGroupDescribeRequestData().setGroupIds(List.of(g, t("nothing"))))));
        // A classic group described by ConsumerGroupDescribe.
        String c = t("classic");
        coordinator((byte) 0, c);
        var join = new JoinGroupRequestData().setGroupId(c).setSessionTimeoutMs(10000).setRebalanceTimeoutMs(10000).setMemberId("").setProtocolType("consumer");
        join.protocols().add(new JoinGroupRequestData.JoinGroupRequestProtocol().setName("range").setMetadata(new byte[] {0}));
        var jr = (JoinGroupResponseData) ask(ApiKeys.JOIN_GROUP, (short) 4, join);
        join.setMemberId(jr.memberId());
        ask(ApiKeys.JOIN_GROUP, (short) 4, join);
        if (has(ApiKeys.CONSUMER_GROUP_DESCRIBE)) say(L, "describe classic v0 " + norm(ask(ApiKeys.CONSUMER_GROUP_DESCRIBE, (short) 0, new ConsumerGroupDescribeRequestData().setGroupIds(List.of(c)))));
    }

    /** ApiVersions at every version the broker lists. */
    static void apiVersions() throws IOException {
        String L = "api-versions";
        for (short v = 0; v <= max(ApiKeys.API_VERSIONS); v++) {
            var d = new ApiVersionsRequestData().setClientSoftwareName("probe").setClientSoftwareVersion("1.0");
            var r = (ApiVersionsResponseData) ask(ApiKeys.API_VERSIONS, v, d);
            String s = "err=" + r.errorCode() + " apis=" + r.apiKeys().size();
            if (v >= 3) {
                s += " supported=" + r.supportedFeatures().stream().map(f -> f.name() + ":" + f.minVersion() + "-" + f.maxVersion()).sorted().toList();
                s += " finalized=" + r.finalizedFeatures().stream().map(f -> f.name() + ":" + f.minVersionLevel() + "-" + f.maxVersionLevel()).sorted().toList();
                s += " epoch=" + (r.finalizedFeaturesEpoch() > 0 ? "E" : Long.toString(r.finalizedFeaturesEpoch()));
                s += " zkMigration=" + r.zkMigrationReady();
            }
            say(L, "v" + v + " " + s);
        }
    }

    /** Produce at every version the broker lists from v3 (acks -1 and 1), with each codec, read back at every Fetch version from v4. */
    static void produceFetchVersions() throws IOException {
        String L = "produce-fetch", tp = t("pf");
        topic(tp, 1);
        for (short v = 3; v <= max(ApiKeys.PRODUCE); v++) say(L, "produce v" + v + " " + produce(v, tp, records((byte) 2, Compression.NONE, 1)));
        for (short v = 4; v <= max(ApiKeys.FETCH); v++) say(L, "fetch v" + v + " " + fetch(v, tp, 0, 1 << 20));
        for (short v = 4; v <= max(ApiKeys.FETCH); v++) say(L, "fetch v" + v + " past end " + fetch(v, tp, 99, 1 << 20));
    }

    public static void main(String[] args) throws Exception {
        String broker = "127.0.0.1:39700";
        String only = null;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-broker" -> broker = args[++i];
                case "-only" -> only = args[++i];
                case "-major" -> major = Integer.parseInt(args[++i]);
                case "-apis" -> apisFrom = args[++i];
                default -> throw new IllegalArgumentException(args[i]);
            }
        }
        String[] hp = broker.split(":");
        host = hp[0];
        port = Integer.parseInt(hp[1]);
        connect();
        if (apisFrom == null) {
            listed = readApis();
        } else {
            // The versions a real broker's run listed (its `all-apis` lines), so a broker under test is asked the same.
            for (String line : java.nio.file.Files.readAllLines(java.nio.file.Path.of(apisFrom))) {
                Matcher m = Pattern.compile("^all-apis: ([0-9]+) ([0-9]+)\\.\\.([0-9]+)$").matcher(line);
                if (m.matches()) listed.put(Integer.parseInt(m.group(1)), new short[] {Short.parseShort(m.group(2)), Short.parseShort(m.group(3))});
            }
        }
        Map<String, ThrowingRunnable> all = new LinkedHashMap<>();
        all.put("all-apis", Probe::allApis);
        all.put("api-versions", Probe::apiVersions);
        all.put("produce-old", Probe::produceOld);
        all.put("produce-old-refused", Probe::produceOldRefused);
        all.put("fetch-old", Probe::fetchOld);
        all.put("fetch-old-txn", Probe::fetchOldTxn);
        all.put("zstd", Probe::zstd);
        all.put("produce-fetch", Probe::produceFetchVersions);
        all.put("list-offsets", Probe::listOffsets);
        all.put("offsets-old", Probe::offsetsOld);
        all.put("admin-old", Probe::adminOld);
        all.put("metadata", Probe::metadata);
        all.put("groups", Probe::groups);
        all.put("find-coordinator", Probe::findCoordinator);
        all.put("txn", Probe::txn);
        all.put("admin-more", Probe::adminMore);
        all.put("consumer-groups", Probe::consumerGroups);
        for (var e : all.entrySet()) {
            if (only != null && !Arrays.asList(only.split(",")).contains(e.getKey())) continue;
            pids.clear();
            try {
                e.getValue().run();
            } catch (Exception ex) {
                say(e.getKey(), "EXCEPTION " + norm(ex));
                try { connect(); } catch (IOException ignored) { }
            }
        }
    }

    interface ThrowingRunnable { void run() throws Exception; }
}
