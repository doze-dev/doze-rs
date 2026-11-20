// The official Java client (Apache kafka-clients) against a running broker: the same contract as the Go program in
// interop/go (one line per scenario: "ok", "skip" with the reason, or "FAIL"), so run.sh's per-major summary counts it.
//
// Run by interop/run.sh with the source launcher (no Maven, no Gradle): java -cp "$(bun interop/java/fetch.ts 4)"
// interop/java/Interop.java -broker 127.0.0.1:9092 [-kafka-major N] [-only name] [-sasl] [-tls-ca ca.pem -broker-ssl host:port]
//
// It compiles against kafka-clients 2.8.2 and 4.4.0 both, so it uses only what both have; the few calls one of them lacks
// (describeTopics' answer, OffsetSpec.maxTimestamp, the group.protocol setting) go through names or reflection.
//
// What runs is decided as in the Go program: the broker is asked for ApiVersions first, and a scenario that needs an API the
// broker does not advertise, or advertises only at versions this client no longer speaks, is printed as skip with the API named.
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.FileInputStream;
import java.lang.reflect.Method;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.security.cert.CertificateFactory;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.function.*;
import javax.net.ssl.*;

import org.apache.kafka.clients.admin.*;
import org.apache.kafka.clients.consumer.*;
import org.apache.kafka.clients.producer.*;
import org.apache.kafka.common.*;
import org.apache.kafka.common.acl.*;
import org.apache.kafka.common.config.ConfigResource;
import org.apache.kafka.common.errors.*;
import org.apache.kafka.common.header.Header;
import org.apache.kafka.common.protocol.ApiKeys;
import org.apache.kafka.common.record.TimestampType;
import org.apache.kafka.common.resource.*;
import org.apache.kafka.common.serialization.*;

public class Interop {
    // ---- What the run is told ----
    static String broker = "127.0.0.1:9092";
    static int kafkaMajor = 4;
    static String clientVersion = clientVersion();
    static String tag = "java-" + clientVersion;
    /** How every client of the run reaches the broker (plaintext unless a SASL or TLS cell says otherwise). */
    static Sec sec = Sec.NONE;
    /** What the broker advertises: API key to {min, max}. */
    static Map<Integer, short[]> offered = new TreeMap<>();

    // API keys the scenarios need.
    static final int PRODUCE = 0, FETCH = 1, LIST_OFFSETS = 2, METADATA = 3, OFFSET_COMMIT = 8, OFFSET_FETCH = 9,
        FIND_COORDINATOR = 10, JOIN_GROUP = 11, HEARTBEAT = 12, LEAVE_GROUP = 13, SYNC_GROUP = 14, DESCRIBE_GROUPS = 15,
        CREATE_TOPICS = 19, DELETE_TOPICS = 20, DELETE_RECORDS = 21, INIT_PRODUCER_ID = 22, ADD_PARTITIONS_TO_TXN = 24,
        ADD_OFFSETS_TO_TXN = 25, END_TXN = 26, TXN_OFFSET_COMMIT = 28, DESCRIBE_ACLS = 29, CREATE_ACLS = 30,
        DELETE_ACLS = 31, DESCRIBE_CONFIGS = 32, CREATE_PARTITIONS = 37, INCREMENTAL_ALTER_CONFIGS = 44,
        DESCRIBE_USER_SCRAM = 50, ALTER_USER_SCRAM = 51, CONSUMER_GROUP_HEARTBEAT = 68,
        LIST_GROUPS = 16, DELETE_GROUPS = 42, OFFSET_DELETE = 47, DESCRIBE_PRODUCERS = 61, DESCRIBE_TRANSACTIONS = 65,
        LIST_TRANSACTIONS = 66, CONSUMER_GROUP_DESCRIBE = 69, OFFSET_FOR_LEADER_EPOCH = 23, DESCRIBE_LOG_DIRS = 35,
        DESCRIBE_CLIENT_QUOTAS = 48, ALTER_CLIENT_QUOTAS = 49;
    /** Names of APIs newer than the 2.8 client, which its ApiKeys does not know. */
    static final Map<Integer, String> NEWER = Map.of(61, "DescribeProducers", 65, "DescribeTransactions", 66, "ListTransactions",
        68, "ConsumerGroupHeartbeat", 69, "ConsumerGroupDescribe");
    static final int[] BASIC = {PRODUCE, FETCH, LIST_OFFSETS, METADATA, CREATE_TOPICS};
    static final int[] GROUP = {FIND_COORDINATOR, JOIN_GROUP, SYNC_GROUP, HEARTBEAT, LEAVE_GROUP, OFFSET_COMMIT, OFFSET_FETCH};
    static final int[] TXN = {INIT_PRODUCER_ID, ADD_PARTITIONS_TO_TXN, END_TXN, FIND_COORDINATOR};

    public static void main(String[] args) throws Exception {
        // kafka-clients logs through slf4j; the simple binding prints INFO (every configuration) unless told: errors only,
        // JAVA_LOG=info (or debug) for more.
        System.setProperty("org.slf4j.simpleLogger.defaultLogLevel", Optional.ofNullable(System.getenv("JAVA_LOG")).orElse("error"));
        System.setProperty("org.slf4j.simpleLogger.logFile", "System.out");
        String only = "", tlsCA = "", sslAddr = "";
        boolean sasl = false;
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-broker" -> broker = args[++i];
                case "-kafka-major" -> kafkaMajor = Integer.parseInt(args[++i]);
                case "-only" -> only = args[++i];
                case "-sasl" -> sasl = true;
                case "-tls-ca" -> tlsCA = args[++i];
                case "-broker-ssl" -> sslAddr = args[++i];
                // The Go program's: a group of named Go clients, which this program has none of.
                case "-members" -> i++;
                default -> { System.out.println("unknown argument " + args[i]); System.exit(2); }
            }
        }
        // The 2.8 client is the one users of Kafka 1 and 2 era brokers run; against a newer major it is not part of the matrix.
        if (clientVersion.startsWith("2.") && kafkaMajor > 2) {
            System.out.printf("skip %-40s kafka-clients %s is run against Kafka 1 and 2 only%n", tag, clientVersion);
            return;
        }
        // ApiVersions is answered before any login, as in Kafka, so it is asked over a bare socket (TLS when the run is -tls).
        offered = askOffered(broker, tlsCA.isEmpty() ? null : tlsCA);
        StringBuilder adv = new StringBuilder();
        for (var e : offered.entrySet()) {
            ApiKeys k = apiKey(e.getKey());
            adv.append(k == null ? "key" + e.getKey() : k.name).append(' ').append(e.getValue()[0]).append('-').append(e.getValue()[1]).append(", ");
        }
        System.out.printf("%s, Kafka %d: the broker advertises %d APIs%n", tag, kafkaMajor, offered.size());
        if (System.getenv("JAVA_APIS") != null) System.out.println(adv);

        List<Scenario> scenarios = new ArrayList<>();
        if (!tlsCA.isEmpty()) {
            String user = "alice", pw = password(), ca = tlsCA, ssl = sslAddr;
            scenarios.add(new Scenario("tls-sasl-ssl-scram-sha-512", BASIC, () -> cell(new Sec("SASL_SSL", "SCRAM-SHA-512", user, pw, ca), broker, true)));
            scenarios.add(new Scenario("tls-ssl", BASIC, () -> cell(new Sec("SSL", null, null, null, ca), ssl, false)));
            scenarios.add(new Scenario("tls-ssl-no-ca", new int[0], () -> refusedTLS(ssl)));
        } else if (sasl) {
            String pw = password();
            sec = new Sec("SASL_PLAINTEXT", "PLAIN", "alice", pw, null);
            for (String mech : List.of("PLAIN", "SCRAM-SHA-256", "SCRAM-SHA-512")) {
                scenarios.add(new Scenario("sasl-" + mech.toLowerCase(), BASIC,
                    () -> cell(new Sec("SASL_PLAINTEXT", mech, "alice", pw, null), broker, true)));
            }
            scenarios.add(new Scenario("sasl-users", new int[]{DESCRIBE_USER_SCRAM, ALTER_USER_SCRAM, PRODUCE, FETCH}, Interop::saslUsers));
        } else {
            for (String codec : List.of("none", "gzip", "snappy", "lz4", "zstd")) {
                scenarios.add(new Scenario("roundtrip-" + codec, BASIC, () -> roundTrip(codec)));
            }
            scenarios.add(new Scenario("idempotent", concat(BASIC, INIT_PRODUCER_ID), Interop::idempotent));
            scenarios.add(new Scenario("txn-commit-abort", concat(BASIC, TXN), Interop::txnCommitAbort));
            scenarios.add(new Scenario("txn-send-offsets", concat(concat(BASIC, TXN), concat(GROUP, ADD_OFFSETS_TO_TXN, TXN_OFFSET_COMMIT)), Interop::txnSendOffsets));
            // KIP-890's second part: the client turns it on when the broker finalizes transaction.version 2 (Kafka 4).
            for (var v2 : List.of(
                    new Scenario("txn-v2-epochs", concat(concat(BASIC, TXN), DESCRIBE_TRANSACTIONS, DESCRIBE_PRODUCERS), Interop::txnV2Epochs),
                    new Scenario("txn-v2-fence", concat(BASIC, TXN), Interop::txnV2Fence),
                    new Scenario("txn-v2-timeout", concat(concat(BASIC, TXN), DESCRIBE_TRANSACTIONS), Interop::txnV2Timeout))) {
                scenarios.add(v2.needClient(4, "transaction v2 (KIP-890)").needVersion(END_TXN, 5, "transaction v2 (KIP-890)"));
            }
            // Topics named by id in a transaction (KIP-516): Produce v13 and TxnOffsetCommit v6, which the 4.4.0 client sends once
            // the broker offers them and its Metadata has given the topics' ids.
            scenarios.add(new Scenario("txn-topic-ids", concat(concat(BASIC, TXN), concat(GROUP, TXN_OFFSET_COMMIT)), Interop::txnTopicIds)
                .needClient(4, "topic ids in Produce and TxnOffsetCommit (KIP-516)").needVersion(END_TXN, 5, "transaction v2 (KIP-890)")
                .needVersion(PRODUCE, 13, "Produce by topic id (KIP-516)").needVersion(TXN_OFFSET_COMMIT, 6, "TxnOffsetCommit by topic id (KIP-516)"));
            // KIP-939's producer setting. The 4.4.0 client has the setting and no two-phase API: it never sets Enable2Pc, and its
            // InitProducerId builder leaves out the unstable v6 (AbstractRequest.Builder), so it sends v5 even to a broker that offers
            // v6. A transaction runs as any other, whatever the broker's settings.
            scenarios.add(new Scenario("txn-two-phase-commit-setting", concat(BASIC, TXN), Interop::txnTwoPhaseSetting)
                .needClient(4, "transaction.two.phase.commit.enable (KIP-939)"));
            scenarios.add(new Scenario("group-classic", concat(BASIC, GROUP), () -> group(false)));
            scenarios.add(new Scenario("group-848", concat(BASIC, CONSUMER_GROUP_HEARTBEAT, OFFSET_COMMIT, OFFSET_FETCH), () -> group(true)));
            scenarios.add(new Scenario("rebalance-commits-classic", concat(BASIC, GROUP), () -> rebalanceCommits(false)));
            scenarios.add(new Scenario("rebalance-commits-848", concat(BASIC, CONSUMER_GROUP_HEARTBEAT, OFFSET_COMMIT, OFFSET_FETCH), () -> rebalanceCommits(true)));
            scenarios.add(new Scenario("admin-topics", concat(BASIC, DELETE_TOPICS), Interop::adminTopics));
            scenarios.add(new Scenario("admin-configs", concat(BASIC, DESCRIBE_CONFIGS, INCREMENTAL_ALTER_CONFIGS), Interop::adminConfigs));
            scenarios.add(new Scenario("admin-create-partitions", concat(BASIC, CREATE_PARTITIONS), Interop::adminCreatePartitions));
            scenarios.add(new Scenario("admin-delete-records", concat(BASIC, DELETE_RECORDS), Interop::adminDeleteRecords));
            scenarios.add(new Scenario("admin-list-offsets", BASIC, Interop::adminListOffsets));
            scenarios.add(new Scenario("admin-max-timestamp", BASIC, Interop::adminMaxTimestamp).needVersion(LIST_OFFSETS, 7, "OffsetSpec.maxTimestamp (KIP-734)"));
            scenarios.add(new Scenario("admin-cluster", new int[]{METADATA}, Interop::adminCluster));
            scenarios.add(new Scenario("admin-acls", concat(BASIC, CREATE_ACLS, DESCRIBE_ACLS, DELETE_ACLS), Interop::adminAcls));
            scenarios.add(new Scenario("admin-scram-users", new int[]{DESCRIBE_USER_SCRAM, ALTER_USER_SCRAM}, Interop::adminScram));
            int[] groupAdmin = {DESCRIBE_GROUPS, LIST_GROUPS, DELETE_GROUPS};
            scenarios.add(new Scenario("admin-groups-classic", concat(concat(BASIC, GROUP), groupAdmin), () -> adminGroups(false)));
            scenarios.add(new Scenario("admin-groups-848", concat(concat(BASIC, groupAdmin), CONSUMER_GROUP_HEARTBEAT, CONSUMER_GROUP_DESCRIBE, OFFSET_COMMIT, OFFSET_FETCH), () -> adminGroups(true)));
            scenarios.add(new Scenario("txn-last-stable-offset", concat(BASIC, TXN), Interop::txnLastStable));
            scenarios.add(new Scenario("produce-fetch-limits", BASIC, Interop::limits));
            scenarios.add(new Scenario("group-848-regex", concat(BASIC, CONSUMER_GROUP_HEARTBEAT, OFFSET_COMMIT, OFFSET_FETCH), Interop::regex848)
                .needClient(4, "SubscriptionPattern (KIP-848 regular expressions)"));
            scenarios.add(new Scenario("admin-transactions", concat(concat(BASIC, TXN), DESCRIBE_PRODUCERS, DESCRIBE_TRANSACTIONS, LIST_TRANSACTIONS), Interop::adminTransactions).needClient(3, "Admin.listTransactions (KIP-664)"));
            scenarios.add(new Scenario("offsets-for-times-log-append", concat(BASIC, DESCRIBE_CONFIGS), Interop::offsetsForTimesLogAppend));
            scenarios.add(new Scenario("offsets-for-times-compressed", BASIC, Interop::offsetsForTimesCompressed));
            scenarios.add(new Scenario("admin-log-dirs", concat(BASIC, DESCRIBE_LOG_DIRS), Interop::adminLogDirs));
            scenarios.add(new Scenario("admin-client-quotas", concat(BASIC, DESCRIBE_CLIENT_QUOTAS, ALTER_CLIENT_QUOTAS), Interop::adminClientQuotas));
            scenarios.add(new Scenario("consumer-validates-position", concat(BASIC, OFFSET_FOR_LEADER_EPOCH), Interop::consumerValidates));
        }
        // zstd came with Produce v7 and Fetch v10 (KIP-110, Kafka 2.1).
        for (Scenario s : scenarios) {
            if (s.name.equals("roundtrip-zstd")) s.needVersion(PRODUCE, 7, "zstd (KIP-110)").needVersion(FETCH, 10, "zstd (KIP-110)");
        }
        boolean failed = false;
        for (Scenario s : scenarios) {
            if (!only.isEmpty() && !only.equals(s.name) && !only.equals(tag + "/" + s.name)) continue;
            String title = tag + "/" + s.name;
            String why = s.skipReason();
            if (why != null) {
                System.out.printf("skip %-40s %s%n", title, why);
                continue;
            }
            long start = System.nanoTime();
            try {
                String note = s.body.call();
                System.out.printf("ok   %-40s %4.1fs  %s%n", title, (System.nanoTime() - start) / 1e9, note);
            } catch (Throwable t) {
                Throwable c = unwrap(t);
                System.out.printf("FAIL %-40s %s%n", title, c.getClass().getSimpleName() + ": " + c.getMessage());
                if (System.getenv("JAVA_TRACE") != null) c.printStackTrace(System.out);
                failed = true;
            }
        }
        // kafka-clients leaves non-daemon threads behind in a few failure paths; the result is printed, so leave now.
        System.out.flush();
        System.exit(failed ? 1 : 0);
    }

    /** The kafka-clients version on the classpath, from the properties file the jar carries (AppInfoParser moved in 4.x). */
    static String clientVersion() {
        try (var in = Interop.class.getClassLoader().getResourceAsStream("kafka/kafka-version.properties")) {
            Properties p = new Properties();
            p.load(in);
            return p.getProperty("version");
        } catch (Exception e) {
            throw new IllegalStateException("no kafka-clients on the classpath", e);
        }
    }

    static String password() {
        return Optional.ofNullable(System.getenv("SASL_PASSWORD")).orElse("alice-secret-pw");
    }

    // ---- ApiVersions ----

    /** ApiVersions v0 over a bare socket: v0 is answered by every broker from 0.10 on, before any SASL handshake. */
    static Map<Integer, short[]> askOffered(String addr, String caFile) throws Exception {
        String[] hp = addr.split(":");
        Socket plain = new Socket(hp[0], Integer.parseInt(hp[1]));
        Socket s = caFile == null ? plain : tlsContext(caFile).getSocketFactory().createSocket(plain, hp[0], Integer.parseInt(hp[1]), true);
        try (s) {
            s.setSoTimeout(15000);
            var out = new DataOutputStream(s.getOutputStream());
            byte[] client = "doze-interop-java".getBytes(StandardCharsets.UTF_8);
            out.writeInt(2 + 2 + 4 + 2 + client.length);
            out.writeShort(18); // ApiVersions
            out.writeShort(0);
            out.writeInt(1);
            out.writeShort(client.length);
            out.write(client);
            out.flush();
            var in = new DataInputStream(s.getInputStream());
            in.readInt();
            in.readInt(); // correlation id
            short err = in.readShort();
            if (err != 0) throw new IllegalStateException("ApiVersions: error " + err);
            int n = in.readInt();
            Map<Integer, short[]> m = new TreeMap<>();
            for (int i = 0; i < n; i++) {
                int key = in.readShort();
                m.put(key, new short[]{in.readShort(), in.readShort()});
            }
            return m;
        }
    }

    static ApiKeys apiKey(int id) {
        try {
            return ApiKeys.forId(id);
        } catch (Exception e) {
            return null;
        }
    }

    static final class Scenario {
        final String name;
        final int[] needs;
        final Callable<String> body;
        final List<Object[]> versions = new ArrayList<>();
        int clientMajor;
        String clientWhat;

        Scenario needClient(int major, String what) {
            clientMajor = major;
            clientWhat = what;
            return this;
        }

        Scenario(String name, int[] needs, Callable<String> body) {
            this.name = name;
            this.needs = needs;
            this.body = body;
        }

        Scenario needVersion(int key, int min, String what) {
            versions.add(new Object[]{key, min, what});
            return this;
        }

        /** Why the scenario cannot run against this broker with this client (null: it can). */
        String skipReason() {
            if (clientMajor > 0 && Integer.parseInt(clientVersion.split("\\.")[0]) < clientMajor) {
                return String.format("kafka-clients %s has no %s", clientVersion, clientWhat);
            }
            List<String> gone = new ArrayList<>(), old = new ArrayList<>(), unknown = new ArrayList<>();
            for (int k : needs) {
                ApiKeys ak = apiKey(k);
                // An API newer than this client (2.8 knows no ConsumerGroupHeartbeat) is named from the list here.
                String n = ak != null ? ak.name : NEWER.getOrDefault(k, "key " + k);
                short[] r = offered.get(k);
                if (r == null) {
                    if (!gone.contains(n)) gone.add(n);
                } else if (ak == null) {
                    if (!unknown.contains(n)) unknown.add(n);
                } else if (r[1] < ak.oldestVersion()) {
                    old.add(String.format("%s v%d-%d (this client speaks v%d and up)", n, r[0], r[1], ak.oldestVersion()));
                }
            }
            if (!gone.isEmpty()) return String.format("Kafka %d does not offer %s", kafkaMajor, String.join(", ", gone));
            if (!old.isEmpty()) return String.format("Kafka %d offers only %s", kafkaMajor, String.join(", ", old));
            if (!unknown.isEmpty()) return String.format("kafka-clients %s has no %s", clientVersion, String.join(", ", unknown));
            for (Object[] v : versions) {
                int key = (Integer) v[0], min = (Integer) v[1];
                short[] r = offered.get(key);
                if (r == null || r[1] < min) {
                    return String.format("Kafka %d offers %s up to v%d; %s needs v%d", kafkaMajor, apiKey(key).name, r == null ? -1 : r[1], v[2], min);
                }
            }
            return null;
        }
    }

    static int[] concat(int[] a, int... b) {
        int[] out = Arrays.copyOf(a, a.length + b.length);
        System.arraycopy(b, 0, out, a.length, b.length);
        return out;
    }

    static boolean has(int key) {
        return offered.containsKey(key);
    }

    static short max(int key) {
        short[] r = offered.get(key);
        return r == null ? -1 : r[1];
    }

    // ---- Security ----

    record Sec(String protocol, String mechanism, String user, String password, String ca) {
        static final Sec NONE = new Sec("PLAINTEXT", null, null, null, null);

        void apply(Properties p) {
            p.put("security.protocol", protocol);
            if (mechanism != null) {
                p.put("sasl.mechanism", mechanism);
                String module = mechanism.equals("PLAIN") ? "org.apache.kafka.common.security.plain.PlainLoginModule"
                    : "org.apache.kafka.common.security.scram.ScramLoginModule";
                p.put("sasl.jaas.config", module + " required username=\"" + user + "\" password=\"" + password + "\";");
            }
            if (ca != null) {
                // A PEM truststore (KIP-651, kafka-clients 2.7 on): this CA and nothing else.
                p.put("ssl.truststore.type", "PEM");
                p.put("ssl.truststore.location", ca);
            }
        }

        Sec as(String u, String pw, String mech) {
            return new Sec(protocol, mech, u, pw, ca);
        }
    }

    static SSLContext tlsContext(String caFile) throws Exception {
        KeyStore ks = KeyStore.getInstance(KeyStore.getDefaultType());
        ks.load(null, null);
        try (var in = new FileInputStream(caFile)) {
            ks.setCertificateEntry("ca", CertificateFactory.getInstance("X.509").generateCertificate(in));
        }
        TrustManagerFactory tmf = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm());
        tmf.init(ks);
        SSLContext ctx = SSLContext.getInstance("TLS");
        ctx.init(null, tmf.getTrustManagers(), null);
        return ctx;
    }

    // ---- Clients ----

    static Properties base(String addr, Sec s) {
        Properties p = new Properties();
        p.put("bootstrap.servers", addr);
        p.put("client.id", "doze-interop-" + tag);
        s.apply(p);
        return p;
    }

    static Admin admin() {
        return admin(broker, sec);
    }

    static Admin admin(String addr, Sec s) {
        Properties p = base(addr, s);
        p.put("default.api.timeout.ms", "30000");
        p.put("request.timeout.ms", "15000");
        return Admin.create(p);
    }

    static KafkaProducer<String, String> producer(Map<String, Object> extra) {
        return producer(broker, sec, extra);
    }

    static KafkaProducer<String, String> producer(String addr, Sec s, Map<String, Object> extra) {
        Properties p = base(addr, s);
        p.put("key.serializer", StringSerializer.class.getName());
        p.put("value.serializer", StringSerializer.class.getName());
        p.put("delivery.timeout.ms", "30000");
        p.put("request.timeout.ms", "15000");
        p.put("max.block.ms", "30000");
        p.putAll(extra);
        return new KafkaProducer<>(p);
    }

    static KafkaConsumer<String, String> consumer(Map<String, Object> extra) {
        return consumer(broker, sec, extra);
    }

    static KafkaConsumer<String, String> consumer(String addr, Sec s, Map<String, Object> extra) {
        Properties p = base(addr, s);
        p.put("key.deserializer", StringDeserializer.class.getName());
        p.put("value.deserializer", StringDeserializer.class.getName());
        p.put("auto.offset.reset", "earliest");
        p.put("default.api.timeout.ms", "30000");
        p.putAll(extra);
        return new KafkaConsumer<>(p);
    }

    static String newTopic(String what) {
        return what + "-" + System.nanoTime();
    }

    static void createTopic(Admin a, String topic, int partitions, Map<String, String> configs) throws Exception {
        NewTopic nt = new NewTopic(topic, partitions, (short) 1);
        if (configs != null) nt.configs(configs);
        a.createTopics(List.of(nt)).all().get(30, TimeUnit.SECONDS);
        // Seen by Metadata with a leader for every partition before anyone produces.
        long deadline = System.currentTimeMillis() + 15000;
        while (true) {
            try {
                TopicDescription d = describeTopic(a, topic);
                if (d.partitions().size() == partitions && d.partitions().stream().allMatch(p -> p.leader() != null && p.leader().id() >= 0)) return;
            } catch (Exception e) {
                if (System.currentTimeMillis() > deadline) throw e;
            }
            if (System.currentTimeMillis() > deadline) throw new java.util.concurrent.TimeoutException(topic + " has no leader for every partition");
            Thread.sleep(50);
        }
    }

    /** describeTopics' answer: allTopicNames() from 3.1, all() before it (removed in 4.0). */
    @SuppressWarnings("unchecked")
    static TopicDescription describeTopic(Admin a, String topic) throws Exception {
        Object r = a.describeTopics(List.of(topic));
        Method m;
        try {
            m = r.getClass().getMethod("allTopicNames");
        } catch (NoSuchMethodException e) {
            m = r.getClass().getMethod("all");
        }
        var f = (KafkaFuture<Map<String, TopicDescription>>) m.invoke(r);
        return f.get(30, TimeUnit.SECONDS).get(topic);
    }

    static Throwable unwrap(Throwable t) {
        while ((t instanceof ExecutionException || t instanceof CompletionException
            || t instanceof java.lang.reflect.InvocationTargetException) && t.getCause() != null) t = t.getCause();
        return t;
    }

    static void check(boolean ok, String fmt, Object... args) {
        if (!ok) throw new AssertionError(String.format(fmt, args));
    }

    /** Reads every partition of topic from the start until want records were read, or until 2 s pass with none. */
    static List<ConsumerRecord<String, String>> readAll(String addr, Sec s, String topic, String isolation, int want) {
        List<ConsumerRecord<String, String>> out = new ArrayList<>();
        try (var c = consumer(addr, s, Map.of("isolation.level", isolation, "enable.auto.commit", "false"))) {
            List<TopicPartition> tps = new ArrayList<>();
            for (PartitionInfo pi : c.partitionsFor(topic, Duration.ofSeconds(30))) tps.add(new TopicPartition(topic, pi.partition()));
            c.assign(tps);
            c.seekToBeginning(tps);
            long last = System.currentTimeMillis(), deadline = last + 60000;
            while (System.currentTimeMillis() < deadline) {
                var recs = c.poll(Duration.ofMillis(200));
                for (var r : recs) out.add(r);
                if (!recs.isEmpty()) last = System.currentTimeMillis();
                if (want >= 0 && out.size() > want) break;
                if (out.size() >= want && System.currentTimeMillis() - last > 2000) break;
            }
        }
        return out;
    }

    static List<ConsumerRecord<String, String>> readAll(String topic, String isolation, int want) {
        return readAll(broker, sec, topic, isolation, want);
    }

    /** Each value read once and only once, and exactly the values of want. */
    static void exactlyOnce(List<ConsumerRecord<String, String>> got, Collection<String> want, String what) {
        Map<String, Integer> seen = new HashMap<>();
        for (var r : got) seen.merge(r.value(), 1, Integer::sum);
        for (String w : want) check(seen.getOrDefault(w, 0) == 1, "%s: %s read %d times, want once", what, w, seen.getOrDefault(w, 0));
        check(seen.size() == want.size(), "%s: read %d distinct values, want %d", what, seen.size(), want.size());
    }

    static List<String> values(String prefix, int from, int n) {
        List<String> out = new ArrayList<>();
        for (int i = from; i < from + n; i++) out.add(prefix + i);
        return out;
    }

    // ---- Produce and consume ----

    static String roundTrip(String codec) throws Exception {
        String topic = newTopic("java-rt-" + codec);
        try (Admin a = admin()) {
            createTopic(a, topic, 3, null);
        }
        int n = 200;
        try (var p = producer(Map.of("compression.type", codec, "linger.ms", "20", "batch.size", "16384"))) {
            for (int i = 0; i < n; i++) {
                var rec = new ProducerRecord<>(topic, "k" + i, "v" + i);
                rec.headers().add("index", Integer.toString(i).getBytes(StandardCharsets.UTF_8));
                rec.headers().add("codec", codec.getBytes(StandardCharsets.UTF_8));
                if (i % 7 == 0) rec.headers().add("empty", new byte[0]);
                p.send(rec);
            }
            // One record with a null key and a null value: kept as nulls, not as empty strings.
            p.send(new ProducerRecord<>(topic, 0, null, null)).get();
            p.flush();
        }
        var got = readAll(topic, "read_uncommitted", n + 1);
        check(got.size() == n + 1, "read %d records, want %d", got.size(), n + 1);
        int nulls = 0;
        Set<Integer> parts = new HashSet<>();
        for (var r : got) {
            parts.add(r.partition());
            if (r.key() == null) {
                check(r.value() == null, "null key with value %s", r.value());
                nulls++;
                continue;
            }
            int i = Integer.parseInt(r.key().substring(1));
            check(r.value().equals("v" + i), "key %s has value %s", r.key(), r.value());
            Header idx = r.headers().lastHeader("index"), c = r.headers().lastHeader("codec");
            check(idx != null && new String(idx.value(), StandardCharsets.UTF_8).equals(Integer.toString(i)), "key %s: header index wrong", r.key());
            check(c != null && new String(c.value(), StandardCharsets.UTF_8).equals(codec), "key %s: header codec wrong", r.key());
            Header e = r.headers().lastHeader("empty");
            check((i % 7 == 0) == (e != null) && (e == null || e.value().length == 0), "key %s: header empty wrong", r.key());
        }
        check(nulls == 1, "%d null records, want 1", nulls);
        check(parts.size() == 3, "keys landed in %d partitions, want 3", parts.size());
        return String.format("%d records with headers over 3 partitions, %s", n + 1, codec);
    }

    static String idempotent() throws Exception {
        String topic = newTopic("java-idem");
        try (Admin a = admin()) {
            createTopic(a, topic, 3, null);
        }
        int n = 2000;
        // enable.idempotence is the default from kafka-clients 3.0; 2.8 is told.
        try (var p = producer(Map.of("enable.idempotence", "true", "acks", "all", "max.in.flight.requests.per.connection", "5", "linger.ms", "5"))) {
            for (int i = 0; i < n; i++) p.send(new ProducerRecord<>(topic, "k" + (i % 50), "v" + i));
            p.flush();
        }
        var got = readAll(topic, "read_uncommitted", n);
        exactlyOnce(got, values("v", 0, n), "idempotent");
        // In order within each key (one partition each).
        Map<String, Integer> last = new HashMap<>();
        for (var r : got) {
            int v = Integer.parseInt(r.value().substring(1));
            Integer prev = last.put(r.key(), v);
            check(prev == null || prev < v, "key %s: %d after %d", r.key(), v, prev);
        }
        return n + " records once each, in order per key";
    }

    // ---- Transactions ----

    static String txnCommitAbort() throws Exception {
        String topic = newTopic("java-txn");
        try (Admin a = admin()) {
            createTopic(a, topic, 3, null);
        }
        try (var p = producer(Map.of("transactional.id", topic + "-tx"))) {
            p.initTransactions();
            int[][] txns = {{0, 10}, {10, 10}, {20, 5}};
            for (int t = 0; t < txns.length; t++) {
                p.beginTransaction();
                for (int i = txns[t][0]; i < txns[t][0] + txns[t][1]; i++) p.send(new ProducerRecord<>(topic, "k" + i, "v" + i));
                p.flush();
                if (t == 1) p.abortTransaction();
                else p.commitTransaction();
            }
        }
        List<String> committed = new ArrayList<>(values("v", 0, 10));
        committed.addAll(values("v", 20, 5));
        exactlyOnce(readAll(topic, "read_committed", 15), committed, "read_committed");
        exactlyOnce(readAll(topic, "read_uncommitted", 25), values("v", 0, 25), "read_uncommitted");
        return "commit 10, abort 10, commit 5: read_committed 15, read_uncommitted 25";
    }

    /** Consume-transform-produce: offsets sent to the transaction; one transaction aborted and redone. */
    @SuppressWarnings("removal") // ConsumerGroupMetadata(String), for a broker before TxnOffsetCommit v3
    /** The newest version of `key` the client negotiated with any broker it has spoken to (`KafkaProducer.apiVersions`). */
    static short negotiated(KafkaProducer<?, ?> p, ApiKeys key) throws Exception {
        var field = KafkaProducer.class.getDeclaredField("apiVersions");
        field.setAccessible(true);
        Object versions = field.get(p);
        var nodes = versions.getClass().getDeclaredField("nodeApiVersions");
        nodes.setAccessible(true);
        short best = -1;
        synchronized (versions) {
            for (Object node : ((Map<?, ?>) nodes.get(versions)).values()) {
                Method m = node.getClass().getMethod("latestUsableVersion", ApiKeys.class);
                best = (short) Math.max(best, (short) m.invoke(node, key));
            }
        }
        return best;
    }

    /** A transaction of the second version that writes and commits consumer offsets, with the client at Produce v13 and
     *  TxnOffsetCommit v6: the records are read once with read_committed, the offsets are the group's after the commit, and an
     *  aborted transaction leaves neither. */
    static String txnTopicIds() throws Exception {
        String in = newTopic("java-ids-in"), out = in.replace("-in-", "-out-"), group = in + "-g";
        try (Admin a = admin()) {
            createTopic(a, in, 2, null);
            createTopic(a, out, 2, null);
        }
        short produceV, offsetsV;
        try (var p = producer(Map.of("transactional.id", in + "-tx"))) {
            p.initTransactions();
            p.beginTransaction();
            for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(out, i % 2, "k" + i, "v" + i)).get(30, TimeUnit.SECONDS);
            p.sendOffsetsToTransaction(Map.of(new TopicPartition(in, 0), new OffsetAndMetadata(4), new TopicPartition(in, 1), new OffsetAndMetadata(6)),
                new ConsumerGroupMetadata(group));
            p.commitTransaction();
            p.beginTransaction();
            p.send(new ProducerRecord<>(out, 0, "gone", "gone")).get(30, TimeUnit.SECONDS);
            p.sendOffsetsToTransaction(Map.of(new TopicPartition(in, 0), new OffsetAndMetadata(99)), new ConsumerGroupMetadata(group));
            p.abortTransaction();
            produceV = negotiated(p, ApiKeys.PRODUCE);
            offsetsV = negotiated(p, ApiKeys.TXN_OFFSET_COMMIT);
        }
        check(produceV == 13 && offsetsV == 6, "negotiated Produce v%d and TxnOffsetCommit v%d, not v13 and v6", produceV, offsetsV);
        exactlyOnce(readAll(out, "read_committed", 10), values("v", 0, 10), "output, read_committed");
        try (Admin a = admin()) {
            var offsets = a.listConsumerGroupOffsets(group).partitionsToOffsetAndMetadata().get(30, TimeUnit.SECONDS);
            long p0 = offsets.get(new TopicPartition(in, 0)).offset(), p1 = offsets.get(new TopicPartition(in, 1)).offset();
            check(p0 == 4 && p1 == 6, "committed %d and %d, not 4 and 6 (the aborted 99 must not show)", p0, p1);
        }
        return "Produce v13 and TxnOffsetCommit v6: 10 records once each, offsets 4 and 6 committed, the aborted transaction left nothing";
    }

    /** A transactional producer with transaction.two.phase.commit.enable=true (and no transaction.timeout.ms, which the
     *  setting forbids) commits and aborts as any other. */
    static String txnTwoPhaseSetting() throws Exception {
        String t = newTopic("java-2pc");
        try (Admin a = admin()) {
            createTopic(a, t, 1, null);
        }
        try (var p = producer(Map.of("transactional.id", t + "-tx", "transaction.two.phase.commit.enable", "true"))) {
            p.initTransactions();
            p.beginTransaction();
            for (int i = 0; i < 5; i++) p.send(new ProducerRecord<>(t, "k" + i, "v" + i));
            p.commitTransaction();
            p.beginTransaction();
            p.send(new ProducerRecord<>(t, "k", "aborted"));
            p.abortTransaction();
        }
        exactlyOnce(readAll(t, "read_committed", 5), values("v", 0, 5), "read_committed");
        return "the broker offers InitProducerId to v" + max(INIT_PRODUCER_ID) + "; 5 committed, the aborted one hidden";
    }

    static String txnSendOffsets() throws Exception {
        String in = newTopic("java-eos-in"), out = in.replace("-in-", "-out-"), group = in + "-g";
        try (Admin a = admin()) {
            createTopic(a, in, 3, null);
            createTopic(a, out, 3, null);
        }
        int n = 200;
        try (var p = producer(Map.of())) {
            for (int i = 0; i < n; i++) p.send(new ProducerRecord<>(in, "k" + i, "v" + i));
            p.flush();
        }
        // Group metadata with the generation needs TxnOffsetCommit v3 (KIP-447, Kafka 2.5); an older broker is given the group
        // id alone, as a user of one does.
        boolean v3 = max(TXN_OFFSET_COMMIT) >= 3;
        int done = 0, aborted = 0, txns = 0;
        try (var c = consumer(Map.of("group.id", group, "enable.auto.commit", "false", "isolation.level", "read_committed", "max.poll.records", "25"));
             var p = producer(Map.of("transactional.id", in + "-tx"))) {
            p.initTransactions();
            c.subscribe(List.of(in));
            long deadline = System.currentTimeMillis() + 60000;
            while (done < n && System.currentTimeMillis() < deadline) {
                var recs = c.poll(Duration.ofMillis(200));
                if (recs.isEmpty()) continue;
                p.beginTransaction();
                Map<TopicPartition, OffsetAndMetadata> next = new HashMap<>();
                Map<TopicPartition, Long> first = new HashMap<>();
                for (var r : recs) {
                    p.send(new ProducerRecord<>(out, r.key(), r.value()));
                    TopicPartition tp = new TopicPartition(r.topic(), r.partition());
                    next.put(tp, new OffsetAndMetadata(r.offset() + 1));
                    first.putIfAbsent(tp, r.offset());
                }
                ConsumerGroupMetadata gm = v3 ? c.groupMetadata() : new ConsumerGroupMetadata(group);
                p.sendOffsetsToTransaction(next, gm);
                txns++;
                if (txns == 2) {
                    p.abortTransaction();
                    aborted += recs.count();
                    for (var e : first.entrySet()) c.seek(e.getKey(), e.getValue());
                    continue;
                }
                p.commitTransaction();
                done += recs.count();
            }
        }
        check(done == n, "processed %d of %d", done, n);
        exactlyOnce(readAll(out, "read_committed", n), values("v", 0, n), "output, read_committed");
        try (Admin a = admin()) {
            var offsets = a.listConsumerGroupOffsets(group).partitionsToOffsetAndMetadata().get(30, TimeUnit.SECONDS);
            Map<TopicPartition, OffsetSpec> ask = new HashMap<>();
            for (int i = 0; i < 3; i++) ask.put(new TopicPartition(in, i), OffsetSpec.latest());
            var ends = a.listOffsets(ask).all().get(30, TimeUnit.SECONDS);
            for (var e : ends.entrySet()) {
                OffsetAndMetadata om = offsets.get(e.getKey());
                check(om != null && om.offset() == e.getValue().offset(), "%s committed %s, end %d", e.getKey(), om, e.getValue().offset());
            }
        }
        return String.format("%d records in %d transactions (one of %d aborted and redone), output once each, input committed to its end%s",
            n, txns, aborted, v3 ? "" : " (group id only: TxnOffsetCommit < v3)");
    }

    // ---- Transactions, KIP-890's second part (transaction.version 2) ----

    /** The producer epoch the coordinator holds for txid (describeTransactions, kafka-clients 3.0 on: reflection for 2.8). */
    static int txnEpoch(Admin a, String txid) throws Exception {
        Object d = ((Map<?, ?>) fut(inv(a, "describeTransactions", List.of(txid)), "all")).get(txid);
        return ((Number) inv(d, "producerEpoch")).intValue();
    }

    static List<String> plus(List<String> a, List<String> b) {
        List<String> out = new ArrayList<>(a);
        out.addAll(b);
        return out;
    }

    /**
     * The broker finalizes transaction.version 2, so the client speaks it: every commit and abort raises the producer epoch by
     * one (EndTxn v5), the partitions are added by the writes themselves, and the partition's producer is at the last marker's epoch.
     */
    static String txnV2Epochs() throws Exception {
        String topic = newTopic("java-txn-v2"), txid = topic + "-tx";
        List<Integer> epochs = new ArrayList<>();
        try (Admin a = admin()) {
            createTopic(a, topic, 2, null);
            Object features = fut(inv(a, "describeFeatures"), "featureMetadata");
            Object tv = ((Map<?, ?>) inv(features, "finalizedFeatures")).get("transaction.version");
            check(tv != null && ((Number) inv(tv, "maxVersionLevel")).intValue() == 2, "transaction.version finalized at %s", tv);
            try (var p = producer(Map.of("transactional.id", txid))) {
                p.initTransactions();
                epochs.add(txnEpoch(a, txid));
                for (int t = 0; t < 3; t++) {
                    p.beginTransaction();
                    for (int i = 0; i < 6; i++) p.send(new ProducerRecord<>(topic, i % 2, "k" + i, "e" + t + "-" + i));
                    p.flush();
                    if (t == 1) p.abortTransaction();
                    else p.commitTransaction();
                    epochs.add(txnEpoch(a, txid));
                }
            }
            for (int i = 1; i < epochs.size(); i++) {
                check(epochs.get(i) == epochs.get(i - 1) + 1, "epochs %s: each end raises the epoch by one", epochs);
            }
            TopicPartition tp = new TopicPartition(topic, 0);
            Object ap = ((List<?>) inv(((Map<?, ?>) fut(inv(a, "describeProducers", List.of(tp)), "all")).get(tp), "activeProducers")).get(0);
            check(((Number) inv(ap, "producerEpoch")).intValue() == epochs.get(epochs.size() - 1), "the partition's producer is at epoch %s, the last end's %s",
                inv(ap, "producerEpoch"), epochs.get(epochs.size() - 1));
        }
        exactlyOnce(readAll(topic, "read_committed", 12), plus(values("e0-", 0, 6), values("e2-", 0, 6)), "read_committed");
        exactlyOnce(readAll(topic, "read_uncommitted", 18), plus(plus(values("e0-", 0, 6), values("e1-", 0, 6)), values("e2-", 0, 6)), "read_uncommitted");
        return "transaction.version 2 finalized; commit, abort, commit raised the epoch " + epochs + "; read_committed 12 of 18";
    }

    /** A second producer of the same transactional id takes it over while the first has a transaction open: the first is fenced for good. */
    static String txnV2Fence() throws Exception {
        String topic = newTopic("java-txn-v2-fence"), txid = topic + "-tx";
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
        }
        String fenced;
        try (var first = producer(Map.of("transactional.id", txid)); var second = producer(Map.of("transactional.id", txid))) {
            first.initTransactions();
            first.beginTransaction();
            first.send(new ProducerRecord<>(topic, "k", "zombie-0"));
            first.flush();
            second.initTransactions();
            try {
                first.send(new ProducerRecord<>(topic, "k", "zombie-1"));
                first.commitTransaction();
                throw new AssertionError("the fenced producer committed");
            } catch (KafkaException e) {
                fenced = e.getClass().getSimpleName();
                check(e instanceof ProducerFencedException || e instanceof InvalidProducerEpochException, "the fenced producer was told %s", e);
            }
            second.beginTransaction();
            second.send(new ProducerRecord<>(topic, "k", "owner-0"));
            second.commitTransaction();
        }
        exactlyOnce(readAll(topic, "read_committed", 1), List.of("owner-0"), "read_committed");
        return "the first producer's open transaction was aborted and its commit refused (" + fenced + "); the new owner's committed";
    }

    /**
     * A transaction that outlives its timeout is aborted by the broker, which raises the epoch. Under transaction.version 2 the
     * producer recovers: its next write is refused with an abortable error, its abort is the retry of the broker's own, and it
     * goes on at the new epoch (the classic protocol fences it for good).
     */
    static String txnV2Timeout() throws Exception {
        String topic = newTopic("java-txn-v2-timeout"), txid = topic + "-tx";
        String refused;
        int before, after;
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            try (var p = producer(Map.of("transactional.id", txid, "transaction.timeout.ms", "1000"))) {
                p.initTransactions();
                before = txnEpoch(a, txid);
                p.beginTransaction();
                p.send(new ProducerRecord<>(topic, "k", "timed-out")).get(30, TimeUnit.SECONDS);
                long deadline = System.currentTimeMillis() + 30000;
                while (txnEpoch(a, txid) == before && System.currentTimeMillis() < deadline) Thread.sleep(200);
                check(txnEpoch(a, txid) == before + 1, "the broker's abort raised the epoch from %d to %d", before, txnEpoch(a, txid));
                try {
                    p.send(new ProducerRecord<>(topic, "k", "late")).get(30, TimeUnit.SECONDS);
                    throw new AssertionError("a write to the timed-out transaction was taken");
                } catch (ExecutionException e) {
                    refused = e.getCause().getClass().getSimpleName();
                }
                p.abortTransaction();
                p.beginTransaction();
                p.send(new ProducerRecord<>(topic, "k", "recovered"));
                p.commitTransaction();
                after = txnEpoch(a, txid);
            }
        }
        exactlyOnce(readAll(topic, "read_committed", 1), List.of("recovered"), "read_committed");
        return String.format("timed out at epoch %d (the next write: %s), aborted, and committed again at %d", before, refused, after - 1);
    }

    // ---- Consumer groups ----

    /** A tally of the values a group read, by value. */
    static final class Tally {
        final ConcurrentHashMap<String, AtomicInteger> seen = new ConcurrentHashMap<>();

        void add(String v) {
            seen.computeIfAbsent(v, k -> new AtomicInteger()).incrementAndGet();
        }

        int distinct() {
            return seen.size();
        }

        int duplicates() {
            int d = 0;
            for (var c : seen.values()) d += c.get() - 1;
            return d;
        }

        void waitFor(int want, List<Member> members, long ms) throws Exception {
            long deadline = System.currentTimeMillis() + ms;
            while (distinct() < want) {
                for (Member m : members) if (m.failed != null) throw new RuntimeException(m.name + ": " + m.failed, m.failed);
                if (System.currentTimeMillis() > deadline) throw new java.util.concurrent.TimeoutException("read " + distinct() + " of " + want + " values");
                Thread.sleep(20);
            }
        }
    }

    /** One consumer of a group on a thread of its own (a KafkaConsumer belongs to one thread). */
    static final class Member implements Runnable {
        final String name;
        final String topic;
        final Tally tally;
        final boolean autoCommit;
        final KafkaConsumer<String, String> c;
        final Set<TopicPartition> owned = ConcurrentHashMap.newKeySet();
        final AtomicInteger reads = new AtomicInteger();
        final AtomicInteger revocations = new AtomicInteger();
        final List<String> commitErrors = Collections.synchronizedList(new ArrayList<>());
        final Thread thread;
        volatile boolean stop;
        volatile Throwable failed;
        /** Commit asynchronously after every poll, so that commits are in flight when the group moves on. */
        final boolean asyncEachPoll;
        /** Slow processing, so that a rebalance finds records being worked on. */
        final int pauseMs;

        Member(String name, String topic, String group, boolean kip848, boolean autoCommit, boolean asyncEachPoll, int pauseMs, Tally tally) {
            this(name, topic, group, kip848, autoCommit, asyncEachPoll, pauseMs, tally, null);
        }

        /** instanceId: a static member (group.instance.id, KIP-345), which does not leave the group when it closes. */
        Member(String name, String topic, String group, boolean kip848, boolean autoCommit, boolean asyncEachPoll, int pauseMs, Tally tally, String instanceId) {
            this.name = name;
            this.topic = topic;
            this.tally = tally;
            this.autoCommit = autoCommit;
            this.asyncEachPoll = asyncEachPoll;
            this.pauseMs = pauseMs;
            Map<String, Object> cfg = new HashMap<>(Map.of("group.id", group, "enable.auto.commit", Boolean.toString(autoCommit),
                "auto.commit.interval.ms", "200", "max.poll.records", "20"));
            if (kip848) {
                // The new consumer (KIP-848): the broker assigns; the classic settings of assignment and session are refused with it.
                cfg.put("group.protocol", "consumer");
            } else {
                cfg.put("session.timeout.ms", "6000");
                cfg.put("heartbeat.interval.ms", "500");
            }
            if (instanceId != null) cfg.put("group.instance.id", instanceId);
            this.c = consumer(cfg);
            this.thread = new Thread(this, name);
        }

        Member start() {
            thread.start();
            return this;
        }

        public void run() {
            try {
                c.subscribe(List.of(topic), new ConsumerRebalanceListener() {
                    public void onPartitionsRevoked(Collection<TopicPartition> parts) {
                        revocations.incrementAndGet();
                        // Commit what was read of the partitions going away before they go: whoever gets them goes on from here.
                        if (!autoCommit && !parts.isEmpty()) {
                            try {
                                c.commitSync();
                            } catch (Exception e) {
                                commitErrors.add("revoke: " + unwrap(e));
                            }
                        }
                        owned.removeAll(parts);
                    }

                    public void onPartitionsAssigned(Collection<TopicPartition> parts) {
                        owned.addAll(parts);
                    }

                    public void onPartitionsLost(Collection<TopicPartition> parts) {
                        owned.removeAll(parts);
                    }
                });
                while (!stop) {
                    ConsumerRecords<String, String> recs;
                    try {
                        recs = c.poll(Duration.ofMillis(100));
                    } catch (WakeupException e) {
                        break;
                    }
                    for (var r : recs) {
                        tally.add(r.value());
                        reads.incrementAndGet();
                    }
                    if (!recs.isEmpty() && pauseMs > 0) Thread.sleep(pauseMs);
                    if (!autoCommit && !recs.isEmpty()) {
                        if (asyncEachPoll) {
                            c.commitAsync((offsets, e) -> {
                                if (e != null) commitErrors.add("async: " + e);
                            });
                        } else {
                            try {
                                c.commitSync();
                            } catch (Exception e) {
                                commitErrors.add("sync: " + unwrap(e));
                            }
                        }
                    }
                }
                if (!autoCommit) {
                    try {
                        c.commitSync();
                    } catch (Exception e) {
                        commitErrors.add("final: " + unwrap(e));
                    }
                }
            } catch (Throwable t) {
                failed = t;
            } finally {
                try {
                    c.close();
                } catch (Throwable t) {
                    if (failed == null) failed = t;
                }
            }
        }

        void stop() throws InterruptedException {
            // No wakeup(): it would land in the final commit as a WakeupException. A poll waits 100 ms at most.
            stop = true;
            thread.join(30000);
        }
    }

    /** Waits until the members between them own every partition, each one at least one, none twice. */
    static void waitBalanced(List<Member> members, int partitions, long ms) throws Exception {
        long deadline = System.currentTimeMillis() + ms;
        while (true) {
            int sum = 0;
            Set<TopicPartition> all = new HashSet<>();
            boolean each = true;
            for (Member m : members) {
                if (m.failed != null) throw new RuntimeException(m.name + ": " + m.failed, m.failed);
                sum += m.owned.size();
                all.addAll(m.owned);
                each &= !m.owned.isEmpty();
            }
            if (each && sum == partitions && all.size() == partitions) return;
            if (System.currentTimeMillis() > deadline) {
                StringBuilder b = new StringBuilder();
                for (Member m : members) b.append(m.name).append('=').append(m.owned).append(' ');
                throw new java.util.concurrent.TimeoutException("not balanced: " + b);
            }
            Thread.sleep(20);
        }
    }

    static void produceValues(String topic, int from, int n) throws Exception {
        try (var p = producer(Map.of("linger.ms", "5"))) {
            for (int i = from; i < from + n; i++) p.send(new ProducerRecord<>(topic, "k" + i, "v" + i));
            p.flush();
        }
    }

    /** Two members join and split the topic; they read, commit and leave; a new member resumes from the commit. */
    static String group(boolean kip848) throws Exception {
        String topic = newTopic(kip848 ? "java-g848" : "java-gc"), group = topic + "-g";
        int parts = 4;
        try (Admin a = admin()) {
            createTopic(a, topic, parts, null);
        }
        Tally one = new Tally();
        Member m1 = new Member("m1", topic, group, kip848, false, false, 0, one).start();
        waitBalanced(List.of(m1), parts, 30000);
        Member m2 = new Member("m2", topic, group, kip848, false, false, 0, one).start();
        List<Member> both = List.of(m1, m2);
        try {
            // The second member's join takes partitions from the first: a rebalance.
            waitBalanced(both, parts, 45000);
            check(m1.revocations.get() > 0 || kip848, "m1 had nothing revoked when m2 joined");
            produceValues(topic, 0, 300);
            one.waitFor(300, both, 60000);
            check(m1.reads.get() > 0 && m2.reads.get() > 0, "m1 read %d, m2 read %d", m1.reads.get(), m2.reads.get());
            // m1 leaves; m2 takes its partitions and reads what comes next.
            m1.stop();
            waitBalanced(List.of(m2), parts, 45000);
            produceValues(topic, 300, 40);
            one.waitFor(340, List.of(m2), 60000);
        } finally {
            m1.stop();
            m2.stop();
        }
        for (Member m : both) {
            check(m.failed == null, "%s: %s", m.name, m.failed);
            check(m.commitErrors.isEmpty(), "%s commit errors: %s", m.name, m.commitErrors);
        }
        check(one.duplicates() == 0, "%d values read twice", one.duplicates());
        // A new member of the group starts where the others committed: exactly the 50 records that came after.
        produceValues(topic, 340, 50);
        Tally two = new Tally();
        Member m3 = new Member("m3", topic, group, kip848, false, false, 0, two).start();
        try {
            two.waitFor(50, List.of(m3), 60000);
            Thread.sleep(500);
        } finally {
            m3.stop();
        }
        check(two.distinct() == 50 && two.duplicates() == 0, "resume read %d distinct (%d twice), want the 50 new", two.distinct(), two.duplicates());
        for (int i = 340; i < 390; i++) check(two.seen.containsKey("v" + i), "resume missed v%d", i);
        return String.format("m1=%d m2=%d, m1 left and m2 took over, a new member resumed with exactly the 50 new", m1.reads.get(), m2.reads.get());
    }

    /**
     * Commits while the group moves: records flow, members join and leave, each commits asynchronously after every poll and
     * synchronously when partitions are taken from it. A commit in flight when the coordinator moves the member to a new
     * epoch arrives at an older one (KIP-1251, doze G848-10), and a commit made while partitions are being revoked is at
     * the epoch before the revocation is acknowledged. Every one must be taken: no commit error, and no record read twice.
     */
    static String rebalanceCommits(boolean kip848) throws Exception {
        String topic = newTopic(kip848 ? "java-rc848" : "java-rcc"), group = topic + "-g";
        int parts = 6, n = 1000; // at least n records, and as many more as the members take to come and go
        try (Admin a = admin()) {
            createTopic(a, topic, parts, null);
        }
        Tally t = new Tally();
        List<Member> all = new ArrayList<>();
        Member a = new Member("a", topic, group, kip848, false, true, 2, t).start();
        all.add(a);
        waitBalanced(List.of(a), parts, 30000);
        // Records flow the whole time the members come and go (20 every 10 ms), and stop once the last move is settled.
        AtomicReference<Exception> prodErr = new AtomicReference<>();
        AtomicBoolean done = new AtomicBoolean();
        AtomicInteger produced = new AtomicInteger();
        Thread producing = new Thread(() -> {
            try (var p = producer(Map.of("linger.ms", "5"))) {
                for (int i = 0; !done.get() || i < n; i++) {
                    p.send(new ProducerRecord<>(topic, "k" + i, "v" + i));
                    produced.set(i + 1);
                    if (i % 20 == 19) Thread.sleep(10);
                }
                p.flush();
            } catch (Exception e) {
                prodErr.set(e);
            }
        });
        producing.start();
        try {
            Thread.sleep(300);
            Member b = new Member("b", topic, group, kip848, false, true, 2, t).start();
            all.add(b);
            waitBalanced(List.of(a, b), parts, 45000);
            Member c = new Member("c", topic, group, kip848, false, true, 2, t).start();
            all.add(c);
            waitBalanced(List.of(a, b, c), parts, 45000);
            b.stop();
            waitBalanced(List.of(a, c), parts, 45000);
            Member d = new Member("d", topic, group, kip848, false, true, 2, t).start();
            all.add(d);
            waitBalanced(List.of(a, c, d), parts, 45000);
            a.stop();
            waitBalanced(List.of(c, d), parts, 45000);
            Thread.sleep(300);
            done.set(true);
            producing.join(60000);
            if (prodErr.get() != null) throw prodErr.get();
            t.waitFor(produced.get(), List.of(c, d), 60000);
        } finally {
            done.set(true);
            for (Member m : all) m.stop();
            producing.join(30000);
        }
        int total = produced.get();
        List<String> errs = new ArrayList<>();
        int revoked = 0;
        StringBuilder split = new StringBuilder();
        for (Member m : all) {
            check(m.failed == null, "%s: %s", m.name, m.failed);
            for (String e : m.commitErrors) errs.add(m.name + " " + e);
            revoked += m.revocations.get();
            split.append(m.name).append('=').append(m.reads.get()).append(' ');
            check(m.reads.get() > 0, "%s read nothing (the churn did not overlap the records)", m.name);
        }
        check(errs.isEmpty(), "%d commits refused: %s", errs.size(), errs.subList(0, Math.min(3, errs.size())));
        check(t.duplicates() == 0, "%d records read twice (a commit was lost)", t.duplicates());
        check(t.distinct() == total, "read %d distinct, want %d", t.distinct(), total);
        return String.format("%d records, four members joined and two left, %d revocations, no commit refused, none read twice (%s)", total, revoked, split.toString().trim());
    }

    // ---- Admin ----

    static String adminTopics() throws Exception {
        String topic = newTopic("java-adm-t");
        try (Admin a = admin()) {
            createTopic(a, topic, 2, Map.of("retention.ms", "3600000", "cleanup.policy", "delete"));
            check(a.listTopics().names().get(30, TimeUnit.SECONDS).contains(topic), "listTopics lacks %s", topic);
            TopicDescription d = describeTopic(a, topic);
            check(d.partitions().size() == 2, "describe: %d partitions", d.partitions().size());
            check(!d.isInternal(), "described as internal");
            // A second create is TopicExistsException.
            try {
                a.createTopics(List.of(new NewTopic(topic, 2, (short) 1))).all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("second create succeeded");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof TopicExistsException, "second create: %s", e.getCause());
            }
            // validateOnly makes nothing.
            String dry = topic + "-dry";
            a.createTopics(List.of(new NewTopic(dry, 1, (short) 1)), new CreateTopicsOptions().validateOnly(true)).all().get(30, TimeUnit.SECONDS);
            check(!a.listTopics().names().get(30, TimeUnit.SECONDS).contains(dry), "validateOnly made the topic");
            a.deleteTopics(List.of(topic)).all().get(30, TimeUnit.SECONDS);
            long deadline = System.currentTimeMillis() + 15000;
            while (a.listTopics().names().get(30, TimeUnit.SECONDS).contains(topic)) {
                check(System.currentTimeMillis() < deadline, "still listed after delete");
                Thread.sleep(50);
            }
            try {
                a.deleteTopics(List.of(topic)).all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("second delete succeeded");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof UnknownTopicOrPartitionException, "second delete: %s", e.getCause());
            }
        }
        return "create (with configs, again: exists, validateOnly), list, describe, delete (again: unknown)";
    }

    static String adminConfigs() throws Exception {
        String topic = newTopic("java-adm-c");
        try (Admin a = admin()) {
            createTopic(a, topic, 1, Map.of("retention.ms", "3600000"));
            ConfigResource res = new ConfigResource(ConfigResource.Type.TOPIC, topic);
            Config c = a.describeConfigs(List.of(res)).all().get(30, TimeUnit.SECONDS).get(res);
            ConfigEntry ret = c.get("retention.ms");
            check(ret != null && ret.value().equals("3600000"), "retention.ms %s", ret);
            check(ret.source() == ConfigEntry.ConfigSource.DYNAMIC_TOPIC_CONFIG, "retention.ms source %s", ret.source());
            ConfigEntry seg = c.get("segment.bytes");
            check(seg != null && seg.source() != ConfigEntry.ConfigSource.DYNAMIC_TOPIC_CONFIG, "segment.bytes %s", seg);
            a.incrementalAlterConfigs(Map.of(res, List.of(
                new AlterConfigOp(new ConfigEntry("retention.ms", null), AlterConfigOp.OpType.DELETE),
                new AlterConfigOp(new ConfigEntry("max.message.bytes", "200000"), AlterConfigOp.OpType.SET),
                new AlterConfigOp(new ConfigEntry("cleanup.policy", "compact"), AlterConfigOp.OpType.APPEND)))).all().get(30, TimeUnit.SECONDS);
            c = a.describeConfigs(List.of(res)).all().get(30, TimeUnit.SECONDS).get(res);
            check(c.get("max.message.bytes").value().equals("200000"), "max.message.bytes %s", c.get("max.message.bytes"));
            check(c.get("retention.ms").source() != ConfigEntry.ConfigSource.DYNAMIC_TOPIC_CONFIG, "retention.ms not deleted: %s", c.get("retention.ms"));
            check(c.get("cleanup.policy").value().equals("delete,compact"), "cleanup.policy %s", c.get("cleanup.policy"));
            // An unknown name is refused (InvalidConfigurationException), and nothing of the request is applied.
            try {
                a.incrementalAlterConfigs(Map.of(res, List.of(
                    new AlterConfigOp(new ConfigEntry("no.such.config", "1"), AlterConfigOp.OpType.SET)))).all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("unknown config accepted");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof InvalidConfigurationException || e.getCause() instanceof InvalidRequestException, "unknown config: %s", e.getCause());
            }
            // The broker's own configuration can be described.
            String id = Integer.toString(a.describeCluster().nodes().get(30, TimeUnit.SECONDS).iterator().next().id());
            ConfigResource br = new ConfigResource(ConfigResource.Type.BROKER, id);
            Config bc = a.describeConfigs(List.of(br)).all().get(30, TimeUnit.SECONDS).get(br);
            check(!bc.entries().isEmpty(), "broker %s has no configs", id);
            return String.format("describe (topic and broker %s, %d entries), incrementalAlterConfigs DELETE/SET/APPEND, unknown name refused", id, bc.entries().size());
        }
    }

    static String adminCreatePartitions() throws Exception {
        String topic = newTopic("java-adm-p");
        try (Admin a = admin()) {
            createTopic(a, topic, 2, null);
            a.createPartitions(Map.of(topic, NewPartitions.increaseTo(5))).all().get(30, TimeUnit.SECONDS);
            long deadline = System.currentTimeMillis() + 15000;
            while (describeTopic(a, topic).partitions().size() != 5) {
                check(System.currentTimeMillis() < deadline, "not 5 partitions");
                Thread.sleep(50);
            }
            try {
                a.createPartitions(Map.of(topic, NewPartitions.increaseTo(3))).all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("decrease accepted");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof InvalidPartitionsException, "decrease: %s", e.getCause());
            }
        }
        // The new partitions take records.
        try (var p = producer(Map.of())) {
            RecordMetadata md = p.send(new ProducerRecord<>(topic, 4, "k", "v")).get(30, TimeUnit.SECONDS);
            check(md.partition() == 4 && md.offset() == 0, "wrote to %d at %d", md.partition(), md.offset());
        }
        return "2 to 5 partitions, a decrease refused, partition 4 written";
    }

    static String adminDeleteRecords() throws Exception {
        String topic = newTopic("java-adm-d");
        TopicPartition tp = new TopicPartition(topic, 0);
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            produceValues(topic, 0, 20);
            var low = a.deleteRecords(Map.of(tp, RecordsToDelete.beforeOffset(10))).lowWatermarks().get(tp).get(30, TimeUnit.SECONDS);
            check(low.lowWatermark() == 10, "low watermark %d", low.lowWatermark());
            long earliest = a.listOffsets(Map.of(tp, OffsetSpec.earliest())).all().get(30, TimeUnit.SECONDS).get(tp).offset();
            check(earliest == 10, "earliest %d after deleting before 10", earliest);
            // Past the end is refused (OffsetOutOfRangeException).
            try {
                a.deleteRecords(Map.of(tp, RecordsToDelete.beforeOffset(100))).all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("delete past the end accepted");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof org.apache.kafka.common.errors.OffsetOutOfRangeException, "past the end: %s", e.getCause());
            }
        }
        var got = readAll(topic, "read_uncommitted", 10);
        check(got.size() == 10 && got.get(0).offset() == 10, "read %d from %s", got.size(), got.isEmpty() ? "-" : got.get(0).offset());
        return "deleted before 10: low watermark and earliest 10, past the end refused, a reader starts at 10";
    }

    static String adminListOffsets() throws Exception {
        String topic = newTopic("java-adm-lo");
        TopicPartition tp = new TopicPartition(topic, 0);
        long base = System.currentTimeMillis() - 100_000;
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            try (var p = producer(Map.of())) {
                // Timestamps 0..9 seconds after base, then one older one (base + 3.5 s) at offset 10.
                for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(topic, 0, base + i * 1000L, "k" + i, "v" + i)).get();
                p.send(new ProducerRecord<>(topic, 0, base + 3500L, "k10", "v10")).get();
            }
            Function<OffsetSpec, ListOffsetsResult.ListOffsetsResultInfo> ask = spec -> {
                try {
                    return a.listOffsets(Map.of(tp, spec)).all().get(30, TimeUnit.SECONDS).get(tp);
                } catch (Exception e) {
                    throw new RuntimeException(e);
                }
            };
            check(ask.apply(OffsetSpec.earliest()).offset() == 0, "earliest");
            check(ask.apply(OffsetSpec.latest()).offset() == 11, "latest %d", ask.apply(OffsetSpec.latest()).offset());
            var t = ask.apply(OffsetSpec.forTimestamp(base + 3000L));
            check(t.offset() == 3 && t.timestamp() == base + 3000L, "forTimestamp(3 s): offset %d ts %d", t.offset(), t.timestamp() - base);
            t = ask.apply(OffsetSpec.forTimestamp(base + 3200L));
            check(t.offset() == 4 && t.timestamp() == base + 4000L, "forTimestamp(3.2 s): offset %d ts %d", t.offset(), t.timestamp() - base);
            t = ask.apply(OffsetSpec.forTimestamp(base + 60_000L));
            check(t.offset() == -1, "forTimestamp after every record: offset %d", t.offset());
        }
        return "earliest 0, latest 11, forTimestamp exact and between, after the last: none";
    }

    static String adminMaxTimestamp() throws Exception {
        String topic = newTopic("java-adm-mt");
        TopicPartition tp = new TopicPartition(topic, 0);
        long base = System.currentTimeMillis() - 100_000;
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            try (var p = producer(Map.of())) {
                long[] ts = {base + 1000, base + 5000, base + 2000, base + 5000, base + 3000};
                for (int i = 0; i < ts.length; i++) p.send(new ProducerRecord<>(topic, 0, ts[i], "k" + i, "v" + i)).get();
            }
            OffsetSpec max = (OffsetSpec) OffsetSpec.class.getMethod("maxTimestamp").invoke(null);
            var r = a.listOffsets(Map.of(tp, max)).all().get(30, TimeUnit.SECONDS).get(tp);
            // The first record with the largest timestamp (Kafka keeps the offset of the first max).
            check(r.offset() == 1 && r.timestamp() == base + 5000, "maxTimestamp: offset %d ts %d", r.offset(), r.timestamp() - base);
        }
        return "the first of two records with the largest timestamp: offset 1";
    }

    static String adminCluster() throws Exception {
        try (Admin a = admin()) {
            var d = a.describeCluster();
            String id = d.clusterId().get(30, TimeUnit.SECONDS);
            Collection<Node> nodes = d.nodes().get(30, TimeUnit.SECONDS);
            Node ctl = d.controller().get(30, TimeUnit.SECONDS);
            check(id != null && !id.isEmpty(), "no cluster id");
            check(nodes.size() == 1, "%d nodes", nodes.size());
            Node n = nodes.iterator().next();
            String[] hp = broker.split(":");
            check(n.port() == Integer.parseInt(hp[1]), "node port %d", n.port());
            check(ctl != null && ctl.id() >= 0, "controller %s", ctl);
            return String.format("cluster %s, node %d at %s:%d, controller %d", id, n.id(), n.host(), n.port(), ctl.id());
        }
    }

    /** DescribeLogDirs: the one directory, every partition of a new topic, and the one written to with its bytes (KIP-113). */
    static String adminLogDirs() throws Exception {
        String topic = newTopic("java-logdirs");
        try (Admin a = admin()) {
            createTopic(a, topic, 2, null);
            produceValues(topic, 0, 20);
            int node = a.describeCluster().nodes().get(30, TimeUnit.SECONDS).iterator().next().id();
            var dirs = a.describeLogDirs(List.of(node)).allDescriptions().get(30, TimeUnit.SECONDS).get(node);
            check(dirs != null && dirs.size() == 1, "log dirs %s", dirs);
            var dir = dirs.values().iterator().next();
            check(dir.error() == null, "dir error %s", dir.error());
            long bytes = 0;
            int partitions = 0;
            for (var e : dir.replicaInfos().entrySet()) {
                if (!e.getKey().topic().equals(topic)) continue;
                partitions++;
                bytes += e.getValue().size();
                check(e.getValue().offsetLag() == 0 && !e.getValue().isFuture(), "replica %s", e.getValue());
            }
            check(partitions == 2 && bytes > 0, "%d partitions, %d bytes", partitions, bytes);
            return String.format("%d partitions, %d bytes", partitions, bytes);
        }
    }

    /**
     * Client quotas (KIP-546 for the APIs, KIP-13 and KIP-219 for the throttling): a producer_byte_rate set for a client id is
     * described back, a producer of that id is throttled (the client's own produce-throttle-time metric says so), and the quota
     * removed is gone.
     */
    static String adminClientQuotas() throws Exception {
        String id = "java-quota-" + System.nanoTime(), topic = newTopic("java-quota");
        var entity = new org.apache.kafka.common.quota.ClientQuotaEntity(Map.of(org.apache.kafka.common.quota.ClientQuotaEntity.CLIENT_ID, id));
        var filter = org.apache.kafka.common.quota.ClientQuotaFilter.containsOnly(
            List.of(org.apache.kafka.common.quota.ClientQuotaFilterComponent.ofEntity(org.apache.kafka.common.quota.ClientQuotaEntity.CLIENT_ID, id)));
        double throttled;
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            a.alterClientQuotas(List.of(new org.apache.kafka.common.quota.ClientQuotaAlteration(entity,
                List.of(new org.apache.kafka.common.quota.ClientQuotaAlteration.Op("producer_byte_rate", 20480.0))))).all().get(30, TimeUnit.SECONDS);
            // Kafka answers DescribeClientQuotas from the broker's copy of the metadata, a moment behind the controller.
            var got = settledQuotas(a, filter, g -> g.containsKey(entity));
            check(Map.of("producer_byte_rate", 20480.0).equals(got.get(entity)), "described %s", got);
            Properties p = base(broker, sec);
            p.put("client.id", id);
            p.put("key.serializer", StringSerializer.class.getName());
            p.put("value.serializer", StringSerializer.class.getName());
            p.put("linger.ms", "0");
            String blob = "x".repeat(20_000);
            try (var producer = new KafkaProducer<String, String>(p)) {
                for (int i = 0; i < 20; i++) producer.send(new ProducerRecord<>(topic, "k", blob)).get(60, TimeUnit.SECONDS);
                throttled = (double) producer.metrics().entrySet().stream()
                    .filter(e -> e.getKey().name().equals("produce-throttle-time-max") && e.getKey().group().equals("producer-metrics"))
                    .findFirst().orElseThrow().getValue().metricValue();
            }
            check(throttled > 0, "400 KB at 20 KiB/s was never throttled");
            a.alterClientQuotas(List.of(new org.apache.kafka.common.quota.ClientQuotaAlteration(entity,
                List.of(new org.apache.kafka.common.quota.ClientQuotaAlteration.Op("producer_byte_rate", null))))).all().get(30, TimeUnit.SECONDS);
            check(settledQuotas(a, filter, Map::isEmpty).isEmpty(), "quota left after removal");
        }
        return String.format("set, described, a producer throttled up to %.0f ms, removed", throttled);
    }

    /** DescribeClientQuotas until `done` holds of the answer, for up to ten seconds. */
    static Map<org.apache.kafka.common.quota.ClientQuotaEntity, Map<String, Double>> settledQuotas(Admin a,
            org.apache.kafka.common.quota.ClientQuotaFilter filter,
            Predicate<Map<org.apache.kafka.common.quota.ClientQuotaEntity, Map<String, Double>>> done) throws Exception {
        var got = a.describeClientQuotas(filter).entities().get(30, TimeUnit.SECONDS);
        for (long end = System.currentTimeMillis() + 10_000; !done.test(got) && System.currentTimeMillis() < end; ) {
            Thread.sleep(100);
            got = a.describeClientQuotas(filter).entities().get(30, TimeUnit.SECONDS);
        }
        return got;
    }

    /**
     * KIP-320: a consumer that seeks to an offset with the leader epoch it was read under validates it with OffsetForLeaderEpoch
     * before it fetches, and goes on from there when the log was not truncated under it.
     */
    static String consumerValidates() throws Exception {
        String topic = newTopic("java-validate");
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
        }
        produceValues(topic, 0, 10);
        TopicPartition tp = new TopicPartition(topic, 0);
        try (var c = consumer(Map.of("enable.auto.commit", "false", "auto.offset.reset", "none"))) {
            c.assign(List.of(tp));
            c.seek(tp, new OffsetAndMetadata(6, Optional.of(0), ""));
            List<ConsumerRecord<String, String>> got = new ArrayList<>();
            long end = System.currentTimeMillis() + 30_000;
            while (got.size() < 4 && System.currentTimeMillis() < end) c.poll(Duration.ofMillis(200)).forEach(got::add);
            check(got.size() == 4 && got.get(0).offset() == 6, "read %d from %s", got.size(), got.isEmpty() ? "nothing" : got.get(0).offset());
            // The leader stamps its epoch into each batch it appends, so a record says the epoch it was written under.
            check(got.get(0).leaderEpoch().equals(Optional.of(0)), "leader epoch %s", got.get(0).leaderEpoch());
        }
        return "sought to offset 6 under epoch 0, validated, read 6 to 9, each under epoch 0";
    }

    static String adminAcls() throws Exception {
        String topic = newTopic("java-adm-acl");
        ResourcePattern res = new ResourcePattern(ResourceType.TOPIC, topic, PatternType.LITERAL);
        ResourcePattern prefixed = new ResourcePattern(ResourceType.GROUP, topic + "-", PatternType.PREFIXED);
        AclBinding read = new AclBinding(res, new AccessControlEntry("User:bob", "*", AclOperation.READ, AclPermissionType.ALLOW));
        AclBinding deny = new AclBinding(res, new AccessControlEntry("User:eve", "*", AclOperation.WRITE, AclPermissionType.DENY));
        AclBinding grp = new AclBinding(prefixed, new AccessControlEntry("User:bob", "*", AclOperation.READ, AclPermissionType.ALLOW));
        try (Admin a = admin()) {
            a.createAcls(List.of(read, deny, grp)).all().get(30, TimeUnit.SECONDS);
            AclBindingFilter mine = new AclBindingFilter(new ResourcePatternFilter(ResourceType.TOPIC, topic, PatternType.LITERAL), AccessControlEntryFilter.ANY);
            Collection<AclBinding> got = a.describeAcls(mine).values().get(30, TimeUnit.SECONDS);
            check(new HashSet<>(got).equals(Set.of(read, deny)), "describe topic: %s", got);
            AclBindingFilter bobs = new AclBindingFilter(ResourcePatternFilter.ANY,
                new AccessControlEntryFilter("User:bob", null, AclOperation.ANY, AclPermissionType.ANY));
            got = a.describeAcls(bobs).values().get(30, TimeUnit.SECONDS);
            check(got.containsAll(Set.of(read, grp)), "describe bob: %s", got);
            // MATCH finds the prefixed group ACL from a name under it.
            AclBindingFilter match = new AclBindingFilter(new ResourcePatternFilter(ResourceType.GROUP, topic + "-x", PatternType.MATCH), AccessControlEntryFilter.ANY);
            got = a.describeAcls(match).values().get(30, TimeUnit.SECONDS);
            check(got.contains(grp), "describe MATCH: %s", got);
            var deleted = a.deleteAcls(List.of(mine, new AclBindingFilter(new ResourcePatternFilter(ResourceType.GROUP, topic + "-", PatternType.PREFIXED), AccessControlEntryFilter.ANY)))
                .all().get(30, TimeUnit.SECONDS);
            check(new HashSet<>(deleted).equals(Set.of(read, deny, grp)), "deleted %s", deleted);
            check(a.describeAcls(mine).values().get(30, TimeUnit.SECONDS).isEmpty(), "ACLs left after delete");
        }
        return "3 created (literal topic allow/deny, prefixed group), described by resource, principal and MATCH, deleted";
    }

    /**
     * Makes (or deletes) a user's two SCRAM credentials, one request each: a KRaft broker (Kafka 4, and doze at every major)
     * keys a request's alterations by user name alone, so two for one user in a request are DUPLICATE_RESOURCE
     * (ScramControlManager.alterCredentials; security.md).
     */
    static void scramBoth(Admin a, String user, String pw, boolean delete) throws Exception {
        for (var m : List.of(ScramMechanism.SCRAM_SHA_256, ScramMechanism.SCRAM_SHA_512)) {
            UserScramCredentialAlteration alt = delete ? new UserScramCredentialDeletion(user, m)
                : new UserScramCredentialUpsertion(user, new ScramCredentialInfo(m, m == ScramMechanism.SCRAM_SHA_256 ? 4096 : 8192), pw);
            a.alterUserScramCredentials(List.of(alt)).all().get(30, TimeUnit.SECONDS);
        }
    }

    static String adminScram() throws Exception {
        String user = "java-" + System.nanoTime(), dup = "";
        try (Admin a = admin()) {
            dup = "";
            if (kafkaMajor == 4) {
                // KRaft: both mechanisms of one user in one request are refused, the user's two alterations alike.
                var r = a.alterUserScramCredentials(List.of(
                    new UserScramCredentialUpsertion(user, new ScramCredentialInfo(ScramMechanism.SCRAM_SHA_256, 4096), "pw"),
                    new UserScramCredentialUpsertion(user, new ScramCredentialInfo(ScramMechanism.SCRAM_SHA_512, 8192), "pw")));
                try {
                    r.all().get(30, TimeUnit.SECONDS);
                    throw new AssertionError("two alterations of one user in a request accepted");
                } catch (ExecutionException e) {
                    check(e.getCause() instanceof DuplicateResourceException, "two of one user: %s", e.getCause());
                }
                dup = "two of one user in a request refused (KRaft), ";
            }
            scramBoth(a, user, "pw", false);
            var d = a.describeUserScramCredentials(List.of(user)).all().get(30, TimeUnit.SECONDS).get(user);
            Set<String> got = new HashSet<>();
            for (var i : d.credentialInfos()) got.add(i.mechanism() + "/" + i.iterations());
            check(got.equals(Set.of("SCRAM_SHA_256/4096", "SCRAM_SHA_512/8192")), "described %s", got);
            scramBoth(a, user, null, true);
            try {
                a.describeUserScramCredentials(List.of(user)).description(user).get(30, TimeUnit.SECONDS);
                throw new AssertionError("deleted user still described");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof ResourceNotFoundException, "describe deleted: %s", e.getCause());
            }
            // Fewer iterations than SCRAM allows (4096) are refused.
            try {
                a.alterUserScramCredentials(List.of(new UserScramCredentialUpsertion(user, new ScramCredentialInfo(ScramMechanism.SCRAM_SHA_256, 100), "x")))
                    .all().get(30, TimeUnit.SECONDS);
                throw new AssertionError("100 iterations accepted");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof UnacceptableCredentialException, "100 iterations: %s", e.getCause());
            }
        }
        return dup + "upsert both mechanisms, describe, delete (then not found), too few iterations refused";
    }

    // ---- Groups and transactions, as an operator sees them (kafka-consumer-groups.sh, kafka-transactions.sh) ----

    static Map<TopicPartition, OffsetAndMetadata> groupOffsets(Admin a, String group) throws Exception {
        return a.listConsumerGroupOffsets(group).partitionsToOffsetAndMetadata().get(30, TimeUnit.SECONDS);
    }

    static ConsumerGroupDescription describeGroup(Admin a, String group) throws Exception {
        return a.describeConsumerGroups(List.of(group)).describedGroups().get(group).get(30, TimeUnit.SECONDS);
    }

    /**
     * One member reads and commits; the group is listed and described; a delete and an admin offset change are refused while
     * it has a member; the member (static, where JoinGroup v5 is offered) is removed by the admin; then the offsets are
     * changed, one partition's deleted, and the group deleted.
     */
    @SuppressWarnings({"deprecation", "removal"}) // state() and listConsumerGroups(), which 2.8 has and 4.x keeps beside groupState() and listGroups()
    static String adminGroups(boolean kip848) throws Exception {
        String topic = newTopic(kip848 ? "java-adm-g848" : "java-adm-gc"), group = topic + "-g";
        TopicPartition p0 = new TopicPartition(topic, 0), p1 = new TopicPartition(topic, 1);
        boolean isStatic = kip848 || max(JOIN_GROUP) >= 5;
        String instance = isStatic ? "static-1" : null;
        List<String> notes = new ArrayList<>();
        try (Admin a = admin()) {
            createTopic(a, topic, 2, null);
            produceValues(topic, 0, 20);
            Tally t = new Tally();
            Member m = new Member("m", topic, group, kip848, false, false, 0, t, instance).start();
            try {
                waitBalanced(List.of(m), 2, 30000);
                t.waitFor(20, List.of(m), 30000);
                long deadline = System.currentTimeMillis() + 15000;
                while (groupOffsets(a, group).values().stream().mapToLong(OffsetAndMetadata::offset).sum() != 20) {
                    check(System.currentTimeMillis() < deadline, "committed %s, want 20 in all", groupOffsets(a, group));
                    Thread.sleep(50);
                }
                check(a.listConsumerGroups().all().get(30, TimeUnit.SECONDS).stream().anyMatch(l -> l.groupId().equals(group)), "not listed");
                ConsumerGroupDescription d = describeGroup(a, group);
                check(d.state().toString().equals("Stable"), "state %s, want Stable", d.state());
                check(d.members().size() == 1, "%d members", d.members().size());
                MemberDescription md = d.members().iterator().next();
                check(md.assignment().topicPartitions().equals(Set.of(p0, p1)), "assignment %s", md.assignment().topicPartitions());
                check(md.groupInstanceId().equals(Optional.ofNullable(instance)), "instance id %s", md.groupInstanceId());
                notes.add("described Stable with the member" + (isStatic ? " (static)" : "") + " and its assignment");
                try {
                    a.deleteConsumerGroups(List.of(group)).all().get(30, TimeUnit.SECONDS);
                    throw new AssertionError("a group with a member deleted");
                } catch (ExecutionException e) {
                    check(e.getCause() instanceof GroupNotEmptyException, "delete with a member: %s", e.getCause());
                }
                try {
                    a.alterConsumerGroupOffsets(group, Map.of(p0, new OffsetAndMetadata(5))).all().get(30, TimeUnit.SECONDS);
                    throw new AssertionError("offsets of a group with a member altered");
                } catch (ExecutionException e) {
                    check(e.getCause() instanceof UnknownMemberIdException, "alter with a member: %s", e.getCause());
                }
                notes.add("delete and alter refused while it has a member");
            } finally {
                m.stop();
            }
            check(m.failed == null && m.commitErrors.isEmpty(), "member: %s %s", m.failed, m.commitErrors);
            if (isStatic) {
                // A static member does not leave when it closes; the admin takes it out (LeaveGroup by instance id).
                check(describeGroup(a, group).members().size() == 1, "the static member left on close");
                a.removeMembersFromConsumerGroup(group, new RemoveMembersFromConsumerGroupOptions(List.of(new MemberToRemove(instance))))
                    .all().get(30, TimeUnit.SECONDS);
                notes.add("the static member removed by the admin");
            }
            long deadline = System.currentTimeMillis() + 15000;
            while (!describeGroup(a, group).members().isEmpty()) {
                check(System.currentTimeMillis() < deadline, "members left: %s", describeGroup(a, group).members());
                Thread.sleep(50);
            }
            check(describeGroup(a, group).state().toString().equals("Empty"), "state %s, want Empty", describeGroup(a, group).state());
            a.alterConsumerGroupOffsets(group, Map.of(p0, new OffsetAndMetadata(5))).all().get(30, TimeUnit.SECONDS);
            check(groupOffsets(a, group).get(p0).offset() == 5, "p0 at %s after the alter", groupOffsets(a, group).get(p0));
            // OffsetDelete came with Kafka 2.4 (KIP-496): at Kafka 1 that step alone is left out.
            if (has(OFFSET_DELETE)) {
                a.deleteConsumerGroupOffsets(group, Set.of(p1)).all().get(30, TimeUnit.SECONDS);
                var left = groupOffsets(a, group);
                check(!left.containsKey(p1) || left.get(p1) == null, "p1 still committed: %s", left);
                notes.add("offsets altered and one deleted");
            } else {
                notes.add("offsets altered (no OffsetDelete at Kafka " + kafkaMajor + ")");
            }
            a.deleteConsumerGroups(List.of(group)).all().get(30, TimeUnit.SECONDS);
            check(a.listConsumerGroups().all().get(30, TimeUnit.SECONDS).stream().noneMatch(l -> l.groupId().equals(group)), "listed after delete");
            check(groupOffsets(a, group).isEmpty(), "offsets after delete: %s", groupOffsets(a, group));
            notes.add("deleted");
        }
        return String.join("; ", notes);
    }

    static long latest(Admin a, TopicPartition tp, IsolationLevel iso) throws Exception {
        return a.listOffsets(Map.of(tp, OffsetSpec.latest()), new ListOffsetsOptions(iso)).all().get(30, TimeUnit.SECONDS).get(tp).offset();
    }

    /** The last stable offset: the end a read_committed reader is given stops at an open transaction's first record. */
    static String txnLastStable() throws Exception {
        String topic = newTopic("java-lso");
        TopicPartition tp = new TopicPartition(topic, 0);
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            try (var p = producer(Map.of("transactional.id", topic + "-tx"))) {
                p.initTransactions();
                p.beginTransaction();
                for (int i = 0; i < 3; i++) p.send(new ProducerRecord<>(topic, 0, "k" + i, "v" + i));
                p.commitTransaction(); // offsets 0-2 and the marker at 3
                p.beginTransaction();
                for (int i = 3; i < 8; i++) p.send(new ProducerRecord<>(topic, 0, "k" + i, "v" + i));
                p.flush(); // offsets 4-8, open
                long lso = latest(a, tp, IsolationLevel.READ_COMMITTED), hw = latest(a, tp, IsolationLevel.READ_UNCOMMITTED);
                check(lso == 4 && hw == 9, "open transaction: read_committed end %d (want 4), read_uncommitted end %d (want 9)", lso, hw);
                try (var c = consumer(Map.of("isolation.level", "read_committed"))) {
                    long end = c.endOffsets(List.of(tp), Duration.ofSeconds(30)).get(tp);
                    check(end == 4, "a read_committed consumer's end offset %d, want 4", end);
                }
                exactlyOnce(readAll(topic, "read_committed", 3), values("v", 0, 3), "read_committed during the open transaction");
                p.commitTransaction();
                lso = latest(a, tp, IsolationLevel.READ_COMMITTED);
                hw = latest(a, tp, IsolationLevel.READ_UNCOMMITTED);
                check(lso == 10 && hw == 10, "after the commit: read_committed end %d, read_uncommitted end %d (want 10)", lso, hw);
            }
        }
        return "an open transaction holds the read_committed end at 4 (listOffsets and endOffsets) while the log ends at 9; 10 and 10 after the commit";
    }

    /** A method by name and argument count, for the Admin calls kafka-clients 2.8 lacks (this file compiles against it). */
    static Object inv(Object o, String name, Object... args) throws Exception {
        for (Method m : o.getClass().getMethods()) {
            if (m.getName().equals(name) && m.getParameterCount() == args.length) {
                m.setAccessible(true);
                return m.invoke(o, args);
            }
        }
        throw new NoSuchMethodException(o.getClass().getName() + "." + name);
    }

    static Object fut(Object result, String name) throws Exception {
        return ((KafkaFuture<?>) inv(result, name)).get(30, TimeUnit.SECONDS);
    }

    static String norm(Object state) {
        return state.toString().replace("_", "").toLowerCase(Locale.ROOT);
    }

    /** listTransactions, describeTransactions and describeProducers (KIP-664, kafka-clients 3.0) on an open, then committed, one. */
    static String adminTransactions() throws Exception {
        String topic = newTopic("java-adm-tx"), txid = topic + "-tx";
        TopicPartition tp = new TopicPartition(topic, 0);
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
            try (var p = producer(Map.of("transactional.id", txid))) {
                p.initTransactions();
                p.beginTransaction();
                for (int i = 0; i < 5; i++) p.send(new ProducerRecord<>(topic, 0, "k" + i, "v" + i));
                p.flush();
                Object mine = null;
                for (Object l : (Collection<?>) fut(inv(a, "listTransactions"), "all")) {
                    if (txid.equals(inv(l, "transactionalId"))) mine = l;
                }
                check(mine != null, "listTransactions lacks %s", txid);
                check(norm(inv(mine, "state")).equals("ongoing"), "listed as %s", inv(mine, "state"));
                long pid = ((Number) inv(mine, "producerId")).longValue();
                Object d = ((Map<?, ?>) fut(inv(a, "describeTransactions", List.of(txid)), "all")).get(txid);
                check(norm(inv(d, "state")).equals("ongoing"), "described as %s", inv(d, "state"));
                check(((Number) inv(d, "producerId")).longValue() == pid, "described producer %s, listed %d", inv(d, "producerId"), pid);
                check(((Set<?>) inv(d, "topicPartitions")).equals(Set.of(tp)), "partitions %s", inv(d, "topicPartitions"));
                Object ps = ((Map<?, ?>) fut(inv(a, "describeProducers", List.of(tp)), "all")).get(tp);
                List<?> active = (List<?>) inv(ps, "activeProducers");
                check(active.size() == 1, "%d active producers", active.size());
                Object ap = active.get(0);
                check(((Number) inv(ap, "producerId")).longValue() == pid, "active producer %s", inv(ap, "producerId"));
                check(inv(ap, "currentTransactionStartOffset").equals(OptionalLong.of(0)), "transaction starts at %s", inv(ap, "currentTransactionStartOffset"));
                check(((Number) inv(ap, "lastSequence")).longValue() == 4, "last sequence %s", inv(ap, "lastSequence"));
                p.commitTransaction();
                d = ((Map<?, ?>) fut(inv(a, "describeTransactions", List.of(txid)), "all")).get(txid);
                check(norm(inv(d, "state")).equals("completecommit"), "after the commit: %s", inv(d, "state"));
                ap = ((List<?>) inv(((Map<?, ?>) fut(inv(a, "describeProducers", List.of(tp)), "all")).get(tp), "activeProducers")).get(0);
                check(inv(ap, "currentTransactionStartOffset").equals(OptionalLong.empty()), "after the commit the transaction starts at %s", inv(ap, "currentTransactionStartOffset"));
            }
        }
        return "an open transaction listed and described Ongoing with its partition, its producer active from offset 0 at sequence 4; CompleteCommit after";
    }

    /**
     * A record over the topic's max.message.bytes is refused (RecordTooLargeException); one under it is read by a consumer
     * whose fetch limits are far smaller, as the first batch of a fetch is returned whatever its size (KIP-74).
     */
    static String limits() throws Exception {
        String topic = newTopic("java-limits");
        try (Admin a = admin()) {
            createTopic(a, topic, 1, Map.of("max.message.bytes", "100000"));
        }
        String big = "x".repeat(150_000), fits = "y".repeat(90_000);
        try (var p = producer(Map.of("max.request.size", "2000000", "compression.type", "none"))) {
            try {
                p.send(new ProducerRecord<>(topic, 0, "big", big)).get(30, TimeUnit.SECONDS);
                throw new AssertionError("a 150 KB record accepted by a topic of max.message.bytes 100000");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof RecordTooLargeException, "150 KB: %s", e.getCause());
            }
            RecordMetadata md = p.send(new ProducerRecord<>(topic, 0, "fits", fits)).get(30, TimeUnit.SECONDS);
            check(md.offset() == 0, "the 90 KB record at %d", md.offset());
        }
        try (var c = consumer(Map.of("max.partition.fetch.bytes", "1024", "fetch.max.bytes", "1024", "enable.auto.commit", "false"))) {
            TopicPartition tp = new TopicPartition(topic, 0);
            c.assign(List.of(tp));
            c.seekToBeginning(List.of(tp));
            ConsumerRecord<String, String> got = null;
            long deadline = System.currentTimeMillis() + 30000;
            while (got == null && System.currentTimeMillis() < deadline) {
                for (var r : c.poll(Duration.ofMillis(200))) got = r;
            }
            check(got != null && got.value().equals(fits), "the 90 KB record not read with 1 KB fetch limits");
        }
        return "150 KB refused by max.message.bytes 100000 (RecordTooLargeException); 90 KB read with fetch limits of 1 KB";
    }

    /** KIP-848 regular expressions, resolved by the broker: matching topics assigned, one made later picked up, others not. */
    static String regex848() throws Exception {
        String prefix = "java-rx-" + System.nanoTime(), other = "other-" + prefix;
        try (Admin a = admin()) {
            createTopic(a, prefix + "-a", 2, null);
            createTopic(a, prefix + "-b", 2, null);
            createTopic(a, other, 1, null);
        }
        Class<?> spc = Class.forName("org.apache.kafka.clients.consumer.SubscriptionPattern");
        Object pattern = spc.getConstructor(String.class).newInstance(prefix + "-.*");
        Set<TopicPartition> owned = ConcurrentHashMap.newKeySet();
        Tally t = new Tally();
        long laterMs;
        try (var c = consumer(Map.of("group.id", prefix + "-g", "group.protocol", "consumer"))) {
            ConsumerRebalanceListener l = new ConsumerRebalanceListener() {
                public void onPartitionsRevoked(Collection<TopicPartition> parts) {
                    owned.removeAll(parts);
                }

                public void onPartitionsAssigned(Collection<TopicPartition> parts) {
                    owned.addAll(parts);
                }
            };
            KafkaConsumer.class.getMethod("subscribe", spc, ConsumerRebalanceListener.class).invoke(c, pattern, l);
            Function<Set<String>, Boolean> topicsAre = want -> {
                Set<String> have = new HashSet<>();
                for (TopicPartition tp : owned) have.add(tp.topic());
                return have.equals(want);
            };
            long deadline = System.currentTimeMillis() + 30000;
            while (!(topicsAre.apply(Set.of(prefix + "-a", prefix + "-b")) && owned.size() == 4)) {
                check(System.currentTimeMillis() < deadline, "assigned %s, want the 4 partitions of -a and -b", owned);
                for (var r : c.poll(Duration.ofMillis(100))) t.add(r.value());
            }
            long made = System.currentTimeMillis();
            try (Admin a = admin()) {
                createTopic(a, prefix + "-c", 1, null);
            }
            deadline = System.currentTimeMillis() + 45000;
            while (owned.size() != 5) {
                check(System.currentTimeMillis() < deadline, "assigned %s, want -c's partition too", owned);
                for (var r : c.poll(Duration.ofMillis(100))) t.add(r.value());
            }
            laterMs = System.currentTimeMillis() - made;
            try (var p = producer(Map.of())) {
                for (String s : List.of("-a", "-b", "-c")) for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(prefix + s, "k" + i, s + i));
                for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(other, "k" + i, "other" + i));
                p.flush();
            }
            deadline = System.currentTimeMillis() + 30000;
            while (t.distinct() < 30 && System.currentTimeMillis() < deadline) {
                for (var r : c.poll(Duration.ofMillis(100))) t.add(r.value());
            }
            for (int i = 0; i < 20; i++) for (var r : c.poll(Duration.ofMillis(50))) t.add(r.value());
        }
        check(t.distinct() == 30 && t.duplicates() == 0, "read %d distinct (%d twice), want the 30 of -a, -b and -c", t.distinct(), t.duplicates());
        for (String v : t.seen.keySet()) check(!v.startsWith("other"), "read %s from a topic the pattern does not match", v);
        return String.format("-a and -b assigned, -c picked up %.1fs after it was made, the topic that does not match left out", laterMs / 1000.0);
    }

    // ---- offsetsForTimes ----

    /** What Kafka answers for a target time: the first record (lowest offset) whose timestamp is at or after it. */
    static void checkOffsetsForTimes(String topic, List<ConsumerRecord<String, String>> recs, List<Long> targets) {
        try (var c = consumer(Map.of("enable.auto.commit", "false"))) {
            TopicPartition tp = new TopicPartition(topic, 0);
            for (long target : targets) {
                ConsumerRecord<String, String> want = null;
                for (var r : recs) if (r.timestamp() >= target && (want == null || r.offset() < want.offset())) want = r;
                OffsetAndTimestamp got = c.offsetsForTimes(Map.of(tp, target), Duration.ofSeconds(30)).get(tp);
                if (want == null) {
                    check(got == null, "target %d: got offset %s, want none", target, got);
                } else {
                    check(got != null && got.offset() == want.offset() && got.timestamp() == want.timestamp(),
                        "target %d: got %s, want offset %d timestamp %d", target, got, want.offset(), want.timestamp());
                }
            }
        }
    }

    static String offsetsForTimesLogAppend() throws Exception {
        String topic = newTopic("java-oft-lat");
        try (Admin a = admin()) {
            createTopic(a, topic, 1, Map.of("message.timestamp.type", "LogAppendTime"));
        }
        long old = System.currentTimeMillis() - 3_600_000;
        try (var p = producer(Map.of("linger.ms", "50"))) {
            // Three batches, a few milliseconds apart; the producer's (old) times are replaced by the broker's clock.
            for (int b = 0; b < 3; b++) {
                for (int i = 0; i < 5; i++) p.send(new ProducerRecord<>(topic, 0, old + i, "k" + (b * 5 + i), "v" + (b * 5 + i)));
                p.flush();
                Thread.sleep(30);
            }
        }
        var recs = readAll(topic, "read_uncommitted", 15);
        check(recs.size() == 15, "read %d", recs.size());
        TreeSet<Long> times = new TreeSet<>();
        for (var r : recs) {
            check(r.timestampType() == TimestampType.LOG_APPEND_TIME, "offset %d: %s", r.offset(), r.timestampType());
            check(r.timestamp() > old + 1_000_000, "offset %d kept the producer's time", r.offset());
            times.add(r.timestamp());
        }
        List<Long> targets = new ArrayList<>(List.of(old, old + 2));
        for (long t : times) targets.addAll(List.of(t - 1, t, t + 1));
        checkOffsetsForTimes(topic, recs, targets);
        return String.format("%d distinct append times over 3 batches, %d targets answered as Kafka does", times.size(), targets.size());
    }

    static String offsetsForTimesCompressed() throws Exception {
        String topic = newTopic("java-oft-gz");
        try (Admin a = admin()) {
            createTopic(a, topic, 1, null);
        }
        long base = System.currentTimeMillis() - 100_000;
        try (var p = producer(Map.of("compression.type", "gzip", "linger.ms", "200", "batch.size", "1000000"))) {
            // Two compressed batches of ten, timestamps 10 ms apart, the second batch's going back below the first's end.
            for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(topic, 0, base + i * 10L, "k" + i, "v" + i));
            p.flush();
            for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(topic, 0, base + 50 + i * 10L, "k" + (10 + i), "v" + (10 + i)));
            p.flush();
        }
        var recs = readAll(topic, "read_uncommitted", 20);
        check(recs.size() == 20, "read %d", recs.size());
        List<Long> targets = new ArrayList<>();
        for (long t = base - 5; t <= base + 150; t += 5) targets.add(t);
        checkOffsetsForTimes(topic, recs, targets);
        return String.format("two gzip batches, %d targets each answered with the record inside the batch", targets.size());
    }

    // ---- SASL and TLS cells ----

    /** Make a topic, write 20 keyed records and read them back, logged in as s; then (refuse) a wrong password must fail. */
    static String cell(Sec s, String addr, boolean refuse) throws Exception {
        String topic = newTopic("java-" + (s.mechanism == null ? "ssl" : s.mechanism.toLowerCase()));
        try (Admin a = admin(addr, s)) {
            createTopic(a, topic, 2, null);
        }
        try (var p = producer(addr, s, Map.of())) {
            for (int i = 0; i < 20; i++) p.send(new ProducerRecord<>(topic, "k" + i, "v" + i));
            p.flush();
        }
        exactlyOnce(readAll(addr, s, topic, "read_uncommitted", 20), values("v", 0, 20), "read back");
        String note = String.format("%s%s: 20 records written and read", s.protocol, s.mechanism == null ? "" : " " + s.mechanism);
        if (refuse) note += "; wrong password: " + refused(addr, s.as(s.user, s.password + "-wrong", s.mechanism));
        return note;
    }

    /** A client with these credentials must be refused with an authentication error within 15 s; returns the words. */
    static String refused(String addr, Sec s) throws Exception {
        long start = System.currentTimeMillis();
        try (Admin a = admin(addr, s)) {
            a.listTopics(new ListTopicsOptions().timeoutMs(15000)).names().get(20, TimeUnit.SECONDS);
            throw new AssertionError("logged in with a wrong password");
        } catch (ExecutionException e) {
            Throwable c = e.getCause();
            check(c instanceof SaslAuthenticationException, "wrong password: %s, want SaslAuthenticationException", c);
            check(System.currentTimeMillis() - start < 15000, "refusal took %d ms", System.currentTimeMillis() - start);
            return c.getClass().getSimpleName();
        }
    }

    static String refusedTLS(String addr) throws Exception {
        // No truststore: the JDK's own CAs, which do not include the throwaway one.
        Sec s = new Sec("SSL", null, null, null, null);
        try (Admin a = admin(addr, s)) {
            a.listTopics(new ListTopicsOptions().timeoutMs(15000)).names().get(20, TimeUnit.SECONDS);
            throw new AssertionError("connected without the CA");
        } catch (ExecutionException e) {
            Throwable c = e.getCause();
            check(c instanceof SslAuthenticationException, "without the CA: %s, want SslAuthenticationException", c);
            return "refused without the CA: " + c.getClass().getSimpleName();
        }
    }

    /** A user made over the API logs in with SCRAM, and once deleted cannot. */
    static String saslUsers() throws Exception {
        String user = "java-erin-" + System.nanoTime();
        try (Admin a = admin()) {
            scramBoth(a, user, "erin-pw", false);
        }
        List<String> notes = new ArrayList<>();
        for (String mech : List.of("SCRAM-SHA-256", "SCRAM-SHA-512")) {
            cell(sec.as(user, "erin-pw", mech), broker, false);
            notes.add(mech);
        }
        // PLAIN cannot log in a user who has only SCRAM verifiers.
        refused(broker, sec.as(user, "erin-pw", "PLAIN"));
        try (Admin a = admin()) {
            scramBoth(a, user, null, true);
        }
        refused(broker, sec.as(user, "erin-pw", "SCRAM-SHA-512"));
        return "a user made with alterUserScramCredentials logs in with " + String.join(" and ", notes) + "; PLAIN refused; deleted, refused";
    }
}
