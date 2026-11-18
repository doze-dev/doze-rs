// Serve.java <host:port> <app> <seconds>: a Kafka Streams word count with group.protocol=streams, for a while, for
// tools/kafka-oracle/streams/tool.sh (kafka-streams-groups.sh needs a running application to describe).
import java.nio.file.Files;
import java.util.*;
import org.apache.kafka.clients.admin.*;
import org.apache.kafka.clients.producer.*;
import org.apache.kafka.common.serialization.*;
import org.apache.kafka.streams.*;
import org.apache.kafka.streams.kstream.*;
import org.apache.kafka.streams.state.Stores;

public class Serve {
    public static void main(String[] a) throws Exception {
        String broker = a[0], app = a[1];
        String in = app + "-in", out = app + "-out";
        try (Admin admin = Admin.create(Map.of("bootstrap.servers", broker))) {
            admin.createTopics(List.of(new NewTopic(in, 2, (short) 1), new NewTopic(out, 1, (short) 1))).all().get();
        }
        Properties pp = new Properties();
        pp.put("bootstrap.servers", broker);
        try (var p = new KafkaProducer<String, String>(pp, new StringSerializer(), new StringSerializer())) {
            for (int i = 0; i < 10; i++) p.send(new ProducerRecord<>(in, "k" + i, "alpha beta alpha")).get();
        }
        StreamsBuilder b = new StreamsBuilder();
        b.<String, String>stream(in, Consumed.with(Serdes.String(), Serdes.String()))
            .flatMapValues(v -> Arrays.asList(v.split(" ")))
            .groupBy((k, w) -> w, Grouped.with("words", Serdes.String(), Serdes.String()))
            .count(Materialized.<String, Long>as(Stores.inMemoryKeyValueStore("counts")).withKeySerde(Serdes.String()).withValueSerde(Serdes.Long()))
            .toStream().to(out, Produced.with(Serdes.String(), Serdes.Long()));
        Properties p = new Properties();
        p.put(StreamsConfig.APPLICATION_ID_CONFIG, app);
        p.put(StreamsConfig.BOOTSTRAP_SERVERS_CONFIG, broker);
        p.put(StreamsConfig.GROUP_PROTOCOL_CONFIG, "streams");
        p.put(StreamsConfig.STATE_DIR_CONFIG, Files.createTempDirectory("serve").toString());
        p.put(StreamsConfig.COMMIT_INTERVAL_MS_CONFIG, 100);
        p.put(StreamsConfig.APPLICATION_SERVER_CONFIG, "localhost:7777");
        p.put("consumer.auto.offset.reset", "earliest");
        KafkaStreams ks = new KafkaStreams(b.build(), p);
        ks.start();
        Thread.sleep(Long.parseLong(a[2]) * 1000);
        ks.close();
        System.out.println("closed");
    }
}
