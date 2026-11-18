// Streams groups (KIP-1071, the streams rebalance protocol) with Apache Kafka Streams 4.4.0 itself: a stateful word count
// with a repartition topic and a changelog, run with group.protocol=streams, and the streams-group Admin calls. Like
// Share.java it is its own program, run by interop/run.sh with the 4.x client only (the same one-line contract: "ok", "skip"
// with the reason, or "FAIL").
//
//   cd interop && java -cp "$(bun java/fetch.ts streams)" java/Streams.java -broker 127.0.0.1:9092 [-kafka-major N] [-only name]
//
// Each scenario is run against a real Apache Kafka 4.4.0 too (docs/port/behaviours/streams-groups.md, "The oracle"), and
// passes there unchanged: what is checked is what both brokers do. The stores are in memory, so RocksDB is not needed.
import java.nio.file.Files;
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.BooleanSupplier;

import org.apache.kafka.clients.admin.*;
import org.apache.kafka.clients.consumer.*;
import org.apache.kafka.clients.producer.*;
import org.apache.kafka.common.*;
import org.apache.kafka.common.config.ConfigResource;
import org.apache.kafka.common.errors.GroupIdNotFoundException;
import org.apache.kafka.common.serialization.*;
import org.apache.kafka.streams.*;
import org.apache.kafka.streams.kstream.*;
import org.apache.kafka.streams.state.Stores;

public class Streams {
    static String broker = "127.0.0.1:9092";
    static int kafkaMajor = 4;
    static String run = Long.toString(System.currentTimeMillis() % 1_000_000, 36);

    interface Body {
        String call() throws Exception;
    }

    public static void main(String[] args) throws Exception {
        System.setProperty("org.slf4j.simpleLogger.defaultLogLevel", Optional.ofNullable(System.getenv("JAVA_LOG")).orElse("error"));
        System.setProperty("org.slf4j.simpleLogger.logFile", "System.out");
        String only = "";
        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "-broker" -> broker = args[++i];
                case "-kafka-major" -> kafkaMajor = Integer.parseInt(args[++i]);
                case "-only" -> only = args[++i];
                case "-members" -> i++;
                default -> { System.out.println("unknown argument " + args[i]); System.exit(2); }
            }
        }
        Map<String, Body> scenarios = new LinkedHashMap<>();
        scenarios.put("streams-wordcount", Streams::wordCount);
        scenarios.put("streams-scale", Streams::scale);
        scenarios.put("streams-restart", Streams::restart);
        scenarios.put("streams-missing-source", Streams::missingSource);
        scenarios.put("streams-admin", Streams::adminCalls);
        boolean failed = false;
        for (var s : scenarios.entrySet()) {
            if (!only.isEmpty() && !only.equals(s.getKey())) continue;
            String title = "java-streams/" + s.getKey();
            if (kafkaMajor < 4) {
                System.out.printf("skip %-40s streams groups are Kafka 4's (KIP-1071)%n", title);
                continue;
            }
            try {
                String detail = s.getValue().call();
                System.out.printf("ok   %-40s %s%n", title, detail);
            } catch (Throwable e) {
                failed = true;
                System.out.printf("FAIL %-40s %s%n", title, e);
            }
        }
        System.exit(failed ? 1 : 0);
    }

    static void check(boolean ok, String what) {
        if (!ok) throw new AssertionError(what);
    }

    static void waitFor(String what, Duration within, BooleanSupplier done) throws InterruptedException {
        long until = System.nanoTime() + within.toNanos();
        while (!done.getAsBoolean()) {
            if (System.nanoTime() > until) throw new AssertionError("timed out waiting for " + what);
            Thread.sleep(200);
        }
    }

    static Admin admin() {
        return Admin.create(Map.of(AdminClientConfig.BOOTSTRAP_SERVERS_CONFIG, broker));
    }

    static void createTopic(Admin a, String name, int partitions) throws Exception {
        a.createTopics(List.of(new NewTopic(name, partitions, (short) 1))).all().get(30, TimeUnit.SECONDS);
    }

    static void produce(String topic, String... lines) throws Exception {
        Properties p = new Properties();
        p.put(ProducerConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        try (var producer = new KafkaProducer<String, String>(p, new StringSerializer(), new StringSerializer())) {
            for (int i = 0; i < lines.length; i++) producer.send(new ProducerRecord<>(topic, "k" + i, lines[i])).get();
        }
    }

    /** The word count: split, re-key by word (a repartition topic), count into an in-memory store (a changelog). */
    static Topology wordCountTopology(String in, String out) {
        StreamsBuilder b = new StreamsBuilder();
        b.<String, String>stream(in, Consumed.with(Serdes.String(), Serdes.String()))
            .flatMapValues(v -> Arrays.asList(v.toLowerCase(Locale.ROOT).split("\\W+")))
            .groupBy((k, w) -> w, Grouped.with("words", Serdes.String(), Serdes.String()))
            .count(Materialized.<String, Long>as(Stores.inMemoryKeyValueStore("counts")).withKeySerde(Serdes.String()).withValueSerde(Serdes.Long()))
            .toStream()
            .to(out, Produced.with(Serdes.String(), Serdes.Long()));
        return b.build();
    }

    static KafkaStreams start(String app, String in, String out, int port) throws Exception {
        Properties p = new Properties();
        p.put(StreamsConfig.APPLICATION_ID_CONFIG, app);
        p.put(StreamsConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        p.put(StreamsConfig.GROUP_PROTOCOL_CONFIG, "streams");
        p.put(StreamsConfig.STATE_DIR_CONFIG, Files.createTempDirectory("doze-streams").toString());
        p.put(StreamsConfig.COMMIT_INTERVAL_MS_CONFIG, 100);
        p.put(StreamsConfig.STATESTORE_CACHE_MAX_BYTES_CONFIG, 0);
        // Port 0: no endpoint. Kafka 4.4.0 cannot answer a member with an endpoint while the topology is not configured (its
        // EndpointToPartitionsManager reads the configured subtopologies that are not there: UNKNOWN_SERVER_ERROR), so the
        // scenario that starts with a missing source topic runs without one (docs/port/behaviours/streams-groups.md, STR-D1).
        if (port != 0) p.put(StreamsConfig.APPLICATION_SERVER_CONFIG, "localhost:" + port);
        p.put(StreamsConfig.consumerPrefix(ConsumerConfig.AUTO_OFFSET_RESET_CONFIG), "earliest");
        KafkaStreams ks = new KafkaStreams(wordCountTopology(in, out), p);
        ks.start();
        return ks;
    }

    static boolean running(KafkaStreams ks) {
        return ks.state() == KafkaStreams.State.RUNNING;
    }

    /** The latest count of each word on the output topic, read from the start for `within`. */
    static Map<String, Long> counts(String out, Map<String, Long> want, Duration within) {
        Properties p = new Properties();
        p.put(ConsumerConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        p.put(ConsumerConfig.GROUP_ID_CONFIG, "reader-" + out + "-" + System.nanoTime());
        p.put(ConsumerConfig.AUTO_OFFSET_RESET_CONFIG, "earliest");
        Map<String, Long> seen = new TreeMap<>();
        try (var c = new KafkaConsumer<String, Long>(p, new StringDeserializer(), new LongDeserializer())) {
            c.subscribe(List.of(out));
            long until = System.nanoTime() + within.toNanos();
            while (System.nanoTime() < until && !seen.entrySet().containsAll(want.entrySet())) {
                for (var r : c.poll(Duration.ofMillis(200))) seen.put(r.key(), r.value());
            }
        }
        return seen;
    }

    static StreamsGroupDescription describe(Admin a, String group) throws Exception {
        return a.describeStreamsGroups(List.of(group)).describedGroups().get(group).get(30, TimeUnit.SECONDS);
    }

    static boolean stable(Admin a, String group, int members) {
        try {
            var d = describe(a, group);
            return d.groupState() == GroupState.STABLE && d.members().size() == members
                && d.members().stream().allMatch(m -> m.memberEpoch() == d.targetAssignmentEpoch());
        } catch (Exception e) {
            return false;
        }
    }

    static int actives(StreamsGroupDescription d) {
        return d.members().stream().mapToInt(m -> m.assignment().activeTasks().stream().mapToInt(t -> t.partitions().size()).sum()).sum();
    }

    static int standbys(StreamsGroupDescription d) {
        return d.members().stream().mapToInt(m -> m.assignment().standbyTasks().stream().mapToInt(t -> t.partitions().size()).sum()).sum();
    }

    static String wordCount() throws Exception {
        String app = "java-streams-wc-" + run, in = app + "-in", out = app + "-out";
        try (Admin a = admin()) {
            createTopic(a, in, 3);
            createTopic(a, out, 3);
            produce(in, "a b a", "c a");
            KafkaStreams ks = start(app, in, out, 7001);
            try {
                waitFor("RUNNING", Duration.ofSeconds(60), () -> running(ks));
                var want = Map.of("a", 3L, "b", 1L, "c", 1L);
                var got = counts(out, want, Duration.ofSeconds(60));
                check(got.entrySet().containsAll(want.entrySet()), "counts " + got);
                waitFor("a stable group", Duration.ofSeconds(30), () -> stable(a, app, 1));
                var d = describe(a, app);
                check(d.subtopologies().size() == 2, "two subtopologies: " + d.subtopologies());
                check(actives(d) == 6, "six active tasks: " + d);
                var topics = a.describeTopics(List.of(app + "-words-repartition", app + "-counts-changelog")).allTopicNames().get();
                check(topics.values().stream().allMatch(t -> t.partitions().size() == 3), "internal topics of three partitions: " + topics);
                var cfg = a.describeConfigs(List.of(new ConfigResource(ConfigResource.Type.TOPIC, app + "-counts-changelog"))).all().get();
                String policy = cfg.values().iterator().next().get("cleanup.policy").value();
                check(policy.equals("compact"), "the changelog is compacted: " + policy);
                var m = d.members().iterator().next();
                check(m.userEndpoint().map(e -> e.port() == 7001).orElse(false), "the member's endpoint: " + m);
                return "counted " + got + ", " + d.groupState() + " at epoch " + d.groupEpoch() + ", changelog " + policy;
            } finally {
                ks.close(Duration.ofSeconds(30));
            }
        }
    }

    static String scale() throws Exception {
        String app = "java-streams-scale-" + run, in = app + "-in", out = app + "-out";
        try (Admin a = admin()) {
            createTopic(a, in, 3);
            createTopic(a, out, 3);
            var standby = new AlterConfigOp(new ConfigEntry("streams.num.standby.replicas", "1"), AlterConfigOp.OpType.SET);
            a.incrementalAlterConfigs(Map.of(new ConfigResource(ConfigResource.Type.GROUP, app), List.of(standby))).all().get();
            List<KafkaStreams> all = new ArrayList<>();
            try {
                for (int n = 1; n <= 3; n++) {
                    all.add(start(app, in, out, 7100 + n));
                    int members = n;
                    waitFor(n + " members stable", Duration.ofSeconds(90), () -> stable(a, app, members));
                }
                var d = describe(a, app);
                check(actives(d) == 6, "six active tasks over three: " + d);
                check(standbys(d) == 3, "a standby for each stateful task: " + d);
                check(d.members().stream().allMatch(m -> !m.assignment().activeTasks().isEmpty()), "each member runs tasks: " + d);
                produce(in, "x y", "x");
                var got = counts(out, Map.of("x", 2L, "y", 1L), Duration.ofSeconds(60));
                check(got.get("x") == 2L, "counted with three instances: " + got);
                all.remove(2).close(Duration.ofSeconds(30));
                waitFor("2 members stable", Duration.ofSeconds(60), () -> stable(a, app, 2));
                // An instance that dies is fenced when its session runs out (streams.session.timeout.ms, 45 s).
                all.remove(1).close(org.apache.kafka.streams.CloseOptions.timeout(Duration.ofSeconds(30))
                    .withGroupMembershipOperation(org.apache.kafka.streams.CloseOptions.GroupMembershipOperation.REMAIN_IN_GROUP));
                waitFor("the dead member fenced", Duration.ofSeconds(90), () -> stable(a, app, 1));
                var after = describe(a, app);
                check(actives(after) == 6, "the survivor runs every task: " + after);
                produce(in, "x");
                var later = counts(out, Map.of("x", 3L), Duration.ofSeconds(60));
                check(later.get("x") == 3L, "counted after the others went: " + later);
                return "1 to 3 instances with standbys (" + standbys(d) + "), one left, one fenced, x=" + later.get("x");
            } finally {
                for (var ks : all) ks.close(Duration.ofSeconds(30));
            }
        }
    }

    static String restart() throws Exception {
        String app = "java-streams-restart-" + run, in = app + "-in", out = app + "-out";
        try (Admin a = admin()) {
            createTopic(a, in, 2);
            createTopic(a, out, 2);
            produce(in, "r r");
            KafkaStreams first = start(app, in, out, 7201);
            waitFor("RUNNING", Duration.ofSeconds(60), () -> running(first));
            check(counts(out, Map.of("r", 2L), Duration.ofSeconds(60)).get("r") == 2L, "first run counts");
            first.close(Duration.ofSeconds(30));
            waitFor("an empty group", Duration.ofSeconds(30), () -> {
                try {
                    return describe(a, app).groupState() == GroupState.EMPTY;
                } catch (Exception e) {
                    return false;
                }
            });
            produce(in, "r");
            KafkaStreams second = start(app, in, out, 7202);
            try {
                waitFor("RUNNING again", Duration.ofSeconds(60), () -> running(second));
                var got = counts(out, Map.of("r", 3L), Duration.ofSeconds(60));
                check(got.get("r") == 3L, "the count goes on from the changelog: " + got);
                return "restarted with its state: r=" + got.get("r");
            } finally {
                second.close(Duration.ofSeconds(30));
            }
        }
    }

    static String missingSource() throws Exception {
        String app = "java-streams-missing-" + run, in = app + "-in", out = app + "-out";
        try (Admin a = admin()) {
            createTopic(a, out, 1);
            KafkaStreams ks = start(app, in, out, 0);
            try {
                waitFor("a group that is not ready", Duration.ofSeconds(60), () -> {
                    try {
                        var d = describe(a, app);
                        return d.groupState() == GroupState.NOT_READY && d.members().size() == 1;
                    } catch (Exception e) {
                        return false;
                    }
                });
                check(!running(ks) || actives(describe(a, app)) == 0, "no task before the source exists");
                createTopic(a, in, 2);
                produce(in, "m");
                var got = counts(out, Map.of("m", 1L), Duration.ofSeconds(90));
                check(got.get("m") == 1L, "counted once the source exists: " + got);
                return "NotReady until the source was made, then m=" + got.get("m");
            } finally {
                ks.close(Duration.ofSeconds(30));
            }
        }
    }

    static String adminCalls() throws Exception {
        String app = "java-streams-admin-" + run, in = app + "-in", out = app + "-out";
        try (Admin a = admin()) {
            createTopic(a, in, 2);
            createTopic(a, out, 1);
            produce(in, "p q");
            KafkaStreams ks = start(app, in, out, 7401);
            waitFor("a stable group", Duration.ofSeconds(60), () -> stable(a, app, 1));
            var listed = a.listGroups(ListGroupsOptions.forStreamsGroups()).all().get();
            check(listed.stream().anyMatch(g -> g.groupId().equals(app) && g.type().equals(Optional.of(GroupType.STREAMS))), "listed as a streams group: " + listed);
            var d = describe(a, app);
            var m = d.members().iterator().next();
            check(!m.processId().isEmpty() && m.clientId().startsWith(app), "the member's process and client: " + m);
            check(d.assignorName().map("sticky"::equals).orElse(true), "the sticky assignor: " + d.assignorName());
            waitFor("offsets committed", Duration.ofSeconds(30), () -> {
                try {
                    return !a.listStreamsGroupOffsets(Map.of(app, new ListStreamsGroupOffsetsSpec())).partitionsToOffsetAndMetadata(app).get().isEmpty();
                } catch (Exception e) {
                    return false;
                }
            });
            ks.close(Duration.ofSeconds(30));
            waitFor("an empty group", Duration.ofSeconds(30), () -> {
                try {
                    return describe(a, app).groupState() == GroupState.EMPTY;
                } catch (Exception e) {
                    return false;
                }
            });
            var reset = Map.of(new TopicPartition(in, 0), new OffsetAndMetadata(0));
            a.alterStreamsGroupOffsets(app, reset).all().get();
            var offsets = a.listStreamsGroupOffsets(Map.of(app, new ListStreamsGroupOffsetsSpec())).partitionsToOffsetAndMetadata(app).get();
            check(offsets.get(new TopicPartition(in, 0)).offset() == 0, "reset to 0: " + offsets);
            a.deleteStreamsGroupOffsets(app, Set.of(new TopicPartition(in, 0))).all().get();
            a.deleteStreamsGroups(List.of(app)).all().get();
            try {
                describe(a, app);
                throw new AssertionError("the group is still there");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof GroupIdNotFoundException, "gone: " + e.getCause());
            }
            return "listed, described, reset, offsets deleted, group deleted";
        }
    }
}
