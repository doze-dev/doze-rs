// Share groups (KIP-932, "Queues for Kafka") with the official Java client: KafkaShareConsumer and the share-group Admin calls,
// which kafka-clients 4.1 made generally available. Interop.java compiles against 2.8 as well, which has none of this, so the
// share scenarios are their own program, run by interop/run.sh with the 4.x client only (the same one-line contract: "ok",
// "skip" with the reason, or "FAIL").
//
//   cd interop && java -cp "$(bun java/fetch.ts 4)" java/Share.java -broker 127.0.0.1:9092 [-kafka-major N] [-only name]
//
// Each scenario is run against a real Apache Kafka 4.4.0 too (docs/port/behaviours/share-groups.md, "The oracle"), and passes
// there unchanged: what is checked is what both brokers do.
import java.time.Duration;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

import org.apache.kafka.clients.admin.*;
import org.apache.kafka.clients.consumer.*;
import org.apache.kafka.clients.producer.*;
import org.apache.kafka.common.*;
import org.apache.kafka.common.config.ConfigResource;
import org.apache.kafka.common.errors.GroupNotEmptyException;
import org.apache.kafka.common.serialization.*;

public class Share {
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
        String tag = "java-share";
        Map<String, Body> scenarios = new LinkedHashMap<>();
        scenarios.put("share-implicit", Share::implicitAck);
        scenarios.put("share-explicit", Share::explicitAck);
        scenarios.put("share-lock-timeout", Share::lockTimeout);
        scenarios.put("share-delivery-limit", Share::deliveryLimit);
        scenarios.put("share-one-partition", Share::onePartitionManyMembers);
        scenarios.put("share-read-committed", Share::readCommitted);
        scenarios.put("share-admin", Share::adminCalls);
        boolean failed = false;
        for (var s : scenarios.entrySet()) {
            if (!only.isEmpty() && !only.equals(s.getKey())) continue;
            String title = tag + "/" + s.getKey();
            if (kafkaMajor < 4) {
                System.out.printf("skip %-40s share groups are Kafka 4's (KIP-932)%n", title);
                continue;
            }
            long start = System.nanoTime();
            try {
                String note = s.getValue().call();
                System.out.printf("ok   %-40s %4.1fs  %s%n", title, (System.nanoTime() - start) / 1e9, note);
            } catch (Throwable t) {
                Throwable c = t instanceof ExecutionException && t.getCause() != null ? t.getCause() : t;
                System.out.printf("FAIL %-40s %s%n", title, c.getClass().getSimpleName() + ": " + c.getMessage());
                if (System.getenv("JAVA_TRACE") != null) c.printStackTrace(System.out);
                failed = true;
            }
        }
        System.out.flush();
        System.exit(failed ? 1 : 0);
    }

    // ---- helpers ----

    static void check(boolean ok, String fmt, Object... args) {
        if (!ok) throw new IllegalStateException(String.format(fmt, args));
    }

    static Admin admin() {
        Properties p = new Properties();
        p.put(AdminClientConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        return Admin.create(p);
    }

    static String topic(String name, int partitions) throws Exception {
        String t = "share-" + name + "-" + run;
        try (Admin a = admin()) {
            a.createTopics(List.of(new NewTopic(t, partitions, (short) 1))).all().get(30, TimeUnit.SECONDS);
        }
        return t;
    }

    /**
     * Sets group configs (IncrementalAlterConfigs on resource type GROUP); the group need not exist yet. Kafka's controller
     * takes the change and its brokers learn it from the metadata log a moment later, so this waits until DescribeConfigs
     * (answered by the broker) shows every value.
     */
    static void groupConfigs(String group, Map<String, String> configs) throws Exception {
        List<AlterConfigOp> ops = new ArrayList<>();
        configs.forEach((k, v) -> ops.add(new AlterConfigOp(new ConfigEntry(k, v), AlterConfigOp.OpType.SET)));
        ConfigResource res = new ConfigResource(ConfigResource.Type.GROUP, group);
        try (Admin a = admin()) {
            a.incrementalAlterConfigs(Map.of(res, ops)).all().get(30, TimeUnit.SECONDS);
            waitFor(() -> {
                try {
                    Config c = a.describeConfigs(List.of(res)).all().get(10, TimeUnit.SECONDS).get(res);
                    return configs.entrySet().stream().allMatch(e -> c.get(e.getKey()) != null && e.getValue().equals(c.get(e.getKey()).value()));
                } catch (Exception e) {
                    return false;
                }
            }, 10000, () -> "the group configs " + configs + " to be in force");
        }
    }

    static void produce(String topic, int from, int n, boolean transactional, boolean commit) throws Exception {
        Properties p = new Properties();
        p.put(ProducerConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        p.put(ProducerConfig.KEY_SERIALIZER_CLASS_CONFIG, StringSerializer.class.getName());
        p.put(ProducerConfig.VALUE_SERIALIZER_CLASS_CONFIG, StringSerializer.class.getName());
        p.put(ProducerConfig.LINGER_MS_CONFIG, "0");
        if (transactional) p.put(ProducerConfig.TRANSACTIONAL_ID_CONFIG, topic + "-tx-" + from);
        try (KafkaProducer<String, String> pr = new KafkaProducer<>(p)) {
            if (transactional) {
                pr.initTransactions();
                pr.beginTransaction();
            }
            for (int i = from; i < from + n; i++) pr.send(new ProducerRecord<>(topic, "k" + i, "v" + i));
            if (transactional) {
                if (commit) pr.commitTransaction();
                else pr.abortTransaction();
            } else {
                pr.flush();
            }
        }
    }

    static KafkaShareConsumer<String, String> consumer(String group, boolean explicit) {
        Properties p = new Properties();
        p.put(ConsumerConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        p.put(ConsumerConfig.GROUP_ID_CONFIG, group);
        p.put(ConsumerConfig.KEY_DESERIALIZER_CLASS_CONFIG, StringDeserializer.class.getName());
        p.put(ConsumerConfig.VALUE_DESERIALIZER_CLASS_CONFIG, StringDeserializer.class.getName());
        p.put(ConsumerConfig.CLIENT_ID_CONFIG, group + "-" + UUID.randomUUID().toString().substring(0, 4));
        if (explicit) p.put("share.acknowledgement.mode", "explicit");
        return new KafkaShareConsumer<>(p);
    }

    /** A share consumer polling on its own thread until stopped, accepting what it gets (implicit mode). */
    static final class Member implements Runnable {
        final KafkaShareConsumer<String, String> c;
        final Map<String, Integer> seen;
        final AtomicInteger reads = new AtomicInteger();
        final AtomicBoolean stop = new AtomicBoolean();
        volatile Throwable failed;
        final Thread thread;

        Member(String group, String topic, Map<String, Integer> seen) {
            this.c = consumer(group, false);
            this.c.subscribe(List.of(topic));
            this.seen = seen;
            this.thread = new Thread(this);
        }

        Member start() {
            thread.start();
            return this;
        }

        public void run() {
            try {
                while (!stop.get()) {
                    for (ConsumerRecord<String, String> r : c.poll(Duration.ofMillis(200))) {
                        seen.merge(r.value(), 1, Integer::sum);
                        reads.incrementAndGet();
                    }
                }
            } catch (Throwable t) {
                failed = t;
            } finally {
                c.close(Duration.ofSeconds(10));
            }
        }

        void stopAndWait() throws InterruptedException {
            stop.set(true);
            thread.join(30000);
        }
    }

    static void waitFor(java.util.function.BooleanSupplier done, long ms, java.util.function.Supplier<String> what) throws InterruptedException {
        long until = System.currentTimeMillis() + ms;
        while (!done.getAsBoolean()) {
            if (System.currentTimeMillis() > until) throw new IllegalStateException("timed out: " + what.get());
            Thread.sleep(100);
        }
    }

    static String earliest(String group) throws Exception {
        groupConfigs(group, Map.of("share.auto.offset.reset", "earliest"));
        return group;
    }

    // ---- scenarios ----

    /** Two members read 200 records of three partitions, accepting each by polling again: every record read exactly once. */
    static String implicitAck() throws Exception {
        String t = topic("implicit", 3), g = earliest(t + "-g");
        produce(t, 0, 200, false, false);
        Map<String, Integer> seen = new ConcurrentHashMap<>();
        Member a = new Member(g, t, seen).start(), b = new Member(g, t, seen).start();
        try {
            waitFor(() -> seen.size() >= 200, 60000, () -> "200 records, have " + seen.size());
            Thread.sleep(1000);
        } finally {
            a.stopAndWait();
            b.stopAndWait();
        }
        check(a.failed == null && b.failed == null, "a: %s, b: %s", a.failed, b.failed);
        long twice = seen.values().stream().filter(n -> n > 1).count();
        check(twice == 0, "%d records read twice", twice);
        return String.format("a read %d, b read %d, none twice", a.reads.get(), b.reads.get());
    }

    /** Explicit acknowledgement: a release comes back with delivery count 2, a reject never comes back, accept is done. */
    static String explicitAck() throws Exception {
        String t = topic("explicit", 1), g = earliest(t + "-g");
        produce(t, 0, 6, false, false);
        Map<String, List<Short>> deliveries = new TreeMap<>();
        try (KafkaShareConsumer<String, String> c = consumer(g, true)) {
            c.subscribe(List.of(t));
            long until = System.currentTimeMillis() + 60000;
            while (System.currentTimeMillis() < until) {
                for (ConsumerRecord<String, String> r : c.poll(Duration.ofMillis(500))) {
                    short dc = r.deliveryCount().orElse((short) -1);
                    deliveries.computeIfAbsent(r.value(), k -> new ArrayList<>()).add(dc);
                    int i = Integer.parseInt(r.value().substring(1));
                    AcknowledgeType type = switch (i % 3) {
                        case 0 -> AcknowledgeType.ACCEPT;
                        case 1 -> dc == 1 ? AcknowledgeType.RELEASE : AcknowledgeType.ACCEPT;
                        default -> AcknowledgeType.REJECT;
                    };
                    c.acknowledge(r, type);
                }
                c.commitSync();
                if (deliveries.size() == 6 && deliveries.get("v1").size() == 2 && deliveries.get("v4").size() == 2) break;
            }
            // A poll more: nothing is left to deliver.
            int more = c.poll(Duration.ofMillis(1500)).count();
            check(more == 0, "%d more records delivered", more);
        }
        check(deliveries.size() == 6, "delivered %s", deliveries);
        for (int i = 0; i < 6; i++) {
            List<Short> want = i % 3 == 1 ? List.of((short) 1, (short) 2) : List.of((short) 1);
            check(deliveries.get("v" + i).equals(want), "v%d delivered %s, want %s", i, deliveries.get("v" + i), want);
        }
        try (Admin a = admin()) {
            var info = a.listShareGroupOffsets(Map.of(g, new ListShareGroupOffsetsSpec())).partitionsToOffsetInfo(g).get(30, TimeUnit.SECONDS);
            var p0 = info.get(new TopicPartition(t, 0));
            check(p0 != null && p0.lag().orElse(-1L) == 0L, "offsets %s", info);
        }
        return "accept once, release then accept (delivery 2), reject never again; lag 0";
    }

    /** A member that holds records and goes quiet loses them when the lock runs out: another member gets them, delivered twice. */
    static String lockTimeout() throws Exception {
        String t = topic("lock", 1), g = earliest(t + "-g");
        groupConfigs(g, Map.of("share.record.lock.duration.ms", "15000"));
        produce(t, 0, 5, false, false);
        try (KafkaShareConsumer<String, String> holder = consumer(g, true); KafkaShareConsumer<String, String> other = consumer(g, true)) {
            holder.subscribe(List.of(t));
            int held = 0;
            long until = System.currentTimeMillis() + 30000;
            while (held == 0 && System.currentTimeMillis() < until) held = holder.poll(Duration.ofMillis(500)).count();
            check(held == 5, "the holder acquired %d", held);
            check(holder.acquisitionLockTimeoutMs().orElse(-1) == 15000, "lock %s", holder.acquisitionLockTimeoutMs());
            other.subscribe(List.of(t));
            long start = System.currentTimeMillis();
            List<Short> counts = new ArrayList<>();
            until = start + 45000;
            while (counts.size() < 5 && System.currentTimeMillis() < until) {
                for (ConsumerRecord<String, String> r : other.poll(Duration.ofMillis(500))) {
                    counts.add(r.deliveryCount().orElse((short) -1));
                    other.acknowledge(r, AcknowledgeType.ACCEPT);
                }
                other.commitSync();
            }
            long waited = System.currentTimeMillis() - start;
            check(counts.size() == 5 && counts.stream().allMatch(c -> c == 2), "the other member got %s", counts);
            check(waited >= 9000, "redelivered after %d ms, before the lock ran out", waited);
            return String.format("redelivered after %.1f s with delivery count 2", waited / 1000.0);
        }
    }

    /** With share.delivery.count.limit 2, a record released on each delivery is archived after the second. */
    static String deliveryLimit() throws Exception {
        String t = topic("limit", 1), g = earliest(t + "-g");
        groupConfigs(g, Map.of("share.delivery.count.limit", "2"));
        produce(t, 0, 3, false, false);
        List<Short> counts = new ArrayList<>();
        try (KafkaShareConsumer<String, String> c = consumer(g, true)) {
            c.subscribe(List.of(t));
            long until = System.currentTimeMillis() + 30000;
            while (System.currentTimeMillis() < until) {
                for (ConsumerRecord<String, String> r : c.poll(Duration.ofMillis(500))) {
                    counts.add(r.deliveryCount().orElse((short) -1));
                    c.acknowledge(r, AcknowledgeType.RELEASE);
                }
                c.commitSync();
                if (counts.size() >= 6) break;
            }
            int more = c.poll(Duration.ofMillis(2000)).count();
            check(more == 0, "%d delivered a third time", more);
        }
        check(counts.equals(List.of((short) 1, (short) 1, (short) 1, (short) 2, (short) 2, (short) 2)), "deliveries %s", counts);
        try (Admin a = admin()) {
            var info = a.listShareGroupOffsets(Map.of(g, new ListShareGroupOffsetsSpec())).partitionsToOffsetInfo(g).get(30, TimeUnit.SECONDS);
            var p0 = info.get(new TopicPartition(t, 0));
            check(p0 != null && p0.lag().orElse(-1L) == 0L, "offsets %s", info);
        }
        return "each record delivered twice, then archived; lag 0";
    }

    /** Three members share one partition: each reads some of 300 records, none is read twice. */
    static String onePartitionManyMembers() throws Exception {
        String t = topic("one", 1), g = earliest(t + "-g");
        Map<String, Integer> seen = new ConcurrentHashMap<>();
        List<Member> ms = List.of(new Member(g, t, seen).start(), new Member(g, t, seen).start(), new Member(g, t, seen).start());
        try {
            // Every member is in the group and assigned the one partition before records come.
            try (Admin a = admin()) {
                waitFor(() -> {
                    try {
                        var d = a.describeShareGroups(List.of(g)).describedGroups().get(g).get(10, TimeUnit.SECONDS);
                        return d.members().size() == 3 && d.members().stream().allMatch(m -> m.assignment().topicPartitions().size() == 1);
                    } catch (Exception e) {
                        return false;
                    }
                }, 60000, () -> "three members sharing the partition");
            }
            for (int i = 0; i < 30; i++) produce(t, i * 10, 10, false, false);
            waitFor(() -> seen.size() >= 300, 60000, () -> "300 records, have " + seen.size());
            Thread.sleep(1000);
        } finally {
            for (Member m : ms) m.stopAndWait();
        }
        for (Member m : ms) check(m.failed == null, "%s", m.failed);
        long twice = seen.values().stream().filter(n -> n > 1).count();
        check(twice == 0, "%d read twice", twice);
        long readers = ms.stream().filter(m -> m.reads.get() > 0).count();
        check(readers >= 2, "only %d of the three members read anything", readers);
        return String.format("reads %d / %d / %d, none twice", ms.get(0).reads.get(), ms.get(1).reads.get(), ms.get(2).reads.get());
    }

    /** share.isolation.level read_committed: an aborted transaction's records are never delivered, a committed one's are. */
    static String readCommitted() throws Exception {
        String t = topic("rc", 1), g = earliest(t + "-g");
        groupConfigs(g, Map.of("share.isolation.level", "read_committed"));
        produce(t, 0, 3, true, false);
        produce(t, 3, 3, true, true);
        produce(t, 6, 2, false, false);
        Set<String> got = new TreeSet<>();
        try (KafkaShareConsumer<String, String> c = consumer(g, false)) {
            c.subscribe(List.of(t));
            long until = System.currentTimeMillis() + 30000;
            while (got.size() < 5 && System.currentTimeMillis() < until) {
                for (ConsumerRecord<String, String> r : c.poll(Duration.ofMillis(500))) got.add(r.value());
            }
            c.poll(Duration.ofMillis(1000)).forEach(r -> got.add(r.value()));
        }
        check(got.equals(new TreeSet<>(List.of("v3", "v4", "v5", "v6", "v7"))), "read %s", got);
        return "the committed transaction and the plain records; the aborted one never";
    }

    /** The admin side: list, describe (members and assignment), offsets, reset to earliest while empty, delete offsets, delete. */
    static String adminCalls() throws Exception {
        String t = topic("admin", 2), g = t + "-g";
        produce(t, 0, 10, false, false);
        Map<String, Integer> seen = new ConcurrentHashMap<>();
        Member m = new Member(g, t, seen).start();
        StringBuilder note = new StringBuilder();
        try (Admin a = admin()) {
            waitFor(() -> {
                try {
                    var d = a.describeShareGroups(List.of(g)).describedGroups().get(g).get(10, TimeUnit.SECONDS);
                    return d.members().size() == 1 && d.members().iterator().next().assignment().topicPartitions().size() == 2;
                } catch (Exception e) {
                    return false;
                }
            }, 60000, () -> "a member with both partitions");
            var listed = a.listGroups(ListGroupsOptions.forShareGroups()).all().get(30, TimeUnit.SECONDS);
            check(listed.stream().anyMatch(l -> l.groupId().equals(g) && l.type().orElse(null) == GroupType.SHARE), "listed %s", listed);
            var d = a.describeShareGroups(List.of(g)).describedGroups().get(g).get(30, TimeUnit.SECONDS);
            check(d.groupState() == GroupState.STABLE, "state %s", d.groupState());
            // A group with a member may not be reset.
            try {
                a.alterShareGroupOffsets(g, Map.of(new TopicPartition(t, 0), 0L)).all().get(30, TimeUnit.SECONDS);
                check(false, "a reset of a group with members was taken");
            } catch (ExecutionException e) {
                check(e.getCause() instanceof GroupNotEmptyException, "reset refused with %s", e.getCause());
            }
            m.stopAndWait();
            waitFor(() -> {
                try {
                    return a.describeShareGroups(List.of(g)).describedGroups().get(g).get(10, TimeUnit.SECONDS).groupState() == GroupState.EMPTY;
                } catch (Exception e) {
                    return false;
                }
            }, 30000, () -> "the group empty");
            // Latest (the default) started both partitions at their end: nothing of the ten was read.
            check(seen.isEmpty(), "read %s", seen.keySet());
            a.alterShareGroupOffsets(g, Map.of(new TopicPartition(t, 0), 0L, new TopicPartition(t, 1), 0L)).all().get(30, TimeUnit.SECONDS);
            var off = a.listShareGroupOffsets(Map.of(g, new ListShareGroupOffsetsSpec())).partitionsToOffsetInfo(g).get(30, TimeUnit.SECONDS);
            check(off.size() == 2 && off.values().stream().allMatch(o -> o.startOffset() == 0), "after the reset %s", off);
            Member again = new Member(g, t, seen).start();
            try {
                waitFor(() -> seen.size() >= 10, 60000, () -> "the ten after the reset, have " + seen.size());
            } finally {
                again.stopAndWait();
            }
            note.append("reset to 0 delivered all ten; ");
            a.deleteShareGroupOffsets(g, Set.of(t)).all().get(30, TimeUnit.SECONDS);
            off = a.listShareGroupOffsets(Map.of(g, new ListShareGroupOffsetsSpec())).partitionsToOffsetInfo(g).get(30, TimeUnit.SECONDS);
            check(off.isEmpty(), "after deleting the offsets %s", off);
            a.deleteShareGroups(List.of(g)).all().get(30, TimeUnit.SECONDS);
            var after = a.listGroups(ListGroupsOptions.forShareGroups()).all().get(30, TimeUnit.SECONDS);
            check(after.stream().noneMatch(l -> l.groupId().equals(g)), "still listed %s", after);
            note.append("offsets deleted, group deleted");
        } finally {
            m.stopAndWait();
        }
        return note.toString();
    }
}
