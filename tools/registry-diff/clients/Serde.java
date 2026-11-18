// Confluent's own serializers and deserializers (Avro, Protobuf, JSON Schema) against a schema registry and a broker:
// registration on and off, use.latest.version, compatibility refusals, the wire header (magic byte, id, Protobuf
// message indexes), and a produce/consume round trip. Prints "ok <case> <detail>" or "FAIL <case> <detail>" per case and
// "header <case> <hex>" for each value's header with the id replaced by its position among the ids seen, so two runs
// (Confluent's registry, doze's) can be compared line for line. See tools/registry-diff/README.md.
//
//   java -cp <jars>:<dir> Serde <registry url> <bootstrap> <prefix>

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.google.protobuf.Descriptors.Descriptor;
import com.google.protobuf.DynamicMessage;
import com.google.protobuf.Message;
import io.confluent.kafka.schemaregistry.avro.AvroSchema;
import io.confluent.kafka.schemaregistry.client.CachedSchemaRegistryClient;
import io.confluent.kafka.schemaregistry.client.SchemaRegistryClient;
import io.confluent.kafka.schemaregistry.json.JsonSchema;
import io.confluent.kafka.schemaregistry.json.JsonSchemaUtils;
import io.confluent.kafka.schemaregistry.protobuf.ProtobufSchema;
import io.confluent.kafka.serializers.KafkaAvroDeserializer;
import io.confluent.kafka.serializers.KafkaAvroSerializer;
import io.confluent.kafka.serializers.json.KafkaJsonSchemaDeserializer;
import io.confluent.kafka.serializers.json.KafkaJsonSchemaSerializer;
import io.confluent.kafka.serializers.protobuf.KafkaProtobufDeserializer;
import io.confluent.kafka.serializers.protobuf.KafkaProtobufSerializer;
import java.nio.ByteBuffer;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import org.apache.avro.Schema;
import org.apache.avro.generic.GenericData;
import org.apache.avro.generic.GenericRecord;
import org.apache.kafka.clients.consumer.ConsumerRecords;
import org.apache.kafka.clients.consumer.KafkaConsumer;
import org.apache.kafka.clients.producer.KafkaProducer;
import org.apache.kafka.clients.producer.ProducerRecord;
import org.apache.kafka.common.serialization.StringDeserializer;
import org.apache.kafka.common.serialization.StringSerializer;

public class Serde {
  static String url;
  static String prefix;
  static int failed = 0;
  static final List<Integer> ids = new ArrayList<>();

  static void ok(String c, String d) {
    System.out.println("ok " + c + " " + d);
  }

  static void fail(String c, String d) {
    failed++;
    System.out.println("FAIL " + c + " " + d);
  }

  static void check(String c, boolean cond, String d) {
    if (cond) ok(c, d);
    else fail(c, d);
  }

  static Map<String, Object> conf(boolean autoRegister, boolean useLatest) {
    Map<String, Object> m = new HashMap<>();
    m.put("schema.registry.url", url);
    m.put("auto.register.schemas", autoRegister);
    m.put("use.latest.version", useLatest);
    return m;
  }

  /** The header of a value: magic byte, id, and the rest of the bytes before the payload, as hex, the id symbolic. */
  static int header(String c, byte[] b, int extra) {
    ByteBuffer buf = ByteBuffer.wrap(b);
    byte magic = buf.get();
    int id = buf.getInt();
    if (!ids.contains(id)) ids.add(id);
    StringBuilder hex = new StringBuilder(String.format("%02x @%d", magic, ids.indexOf(id) + 1));
    for (int i = 5; i < 5 + extra && i < b.length; i++) hex.append(String.format(" %02x", b[i]));
    System.out.println("header " + c + " " + hex);
    check(c + "-magic", magic == 0, "magic " + magic);
    return id;
  }

  static String subject(String s) {
    return prefix + s;
  }

  public static void main(String[] args) throws Exception {
    url = args[0];
    String bootstrap = args[1];
    prefix = args[2];
    SchemaRegistryClient client = new CachedSchemaRegistryClient(url, 100);

    // ---- Avro ----
    Schema user =
        new Schema.Parser()
            .parse(
                "{\"type\":\"record\",\"name\":\"User\",\"namespace\":\"e2e\",\"fields\":[{\"name\":\"id\",\"type\":\"long\"},{\"name\":\"name\",\"type\":\"string\"}]}");
    GenericRecord rec = new GenericData.Record(user);
    rec.put("id", 42L);
    rec.put("name", "ada");
    String avroTopic = prefix + "avro";
    try (KafkaAvroSerializer ser = new KafkaAvroSerializer();
        KafkaAvroDeserializer de = new KafkaAvroDeserializer()) {
      ser.configure(conf(true, false), false);
      de.configure(conf(true, false), false);
      byte[] b = ser.serialize(avroTopic, rec);
      int id = header("avro-auto-register", b, 0);
      int latest = client.getLatestSchemaMetadata(avroTopic + "-value").getId();
      check("avro-auto-register-id", id == latest, "header id is the subject's latest");
      Object back = de.deserialize(avroTopic, b);
      check("avro-round-trip", rec.toString().equals(back.toString()), String.valueOf(back));
      byte[] again = ser.serialize(avroTopic, rec);
      check("avro-same-id-again", ByteBuffer.wrap(again, 1, 4).getInt() == id, "one id");
    } catch (Exception e) {
      fail("avro-auto-register", e.toString());
    }
    // A compatible evolution (a field with a default) registers; an incompatible one is refused with 409.
    try (KafkaAvroSerializer ser = new KafkaAvroSerializer()) {
      ser.configure(conf(true, false), false);
      Schema v2 =
          new Schema.Parser()
              .parse(
                  "{\"type\":\"record\",\"name\":\"User\",\"namespace\":\"e2e\",\"fields\":[{\"name\":\"id\",\"type\":\"long\"},{\"name\":\"name\",\"type\":\"string\"},{\"name\":\"email\",\"type\":[\"null\",\"string\"],\"default\":null}]}");
      GenericRecord r2 = new GenericData.Record(v2);
      r2.put("id", 1L);
      r2.put("name", "b");
      header("avro-evolve", ser.serialize(avroTopic, r2), 0);
      ok("avro-evolve", "v2 registered");
      Schema bad =
          new Schema.Parser()
              .parse(
                  "{\"type\":\"record\",\"name\":\"User\",\"namespace\":\"e2e\",\"fields\":[{\"name\":\"id\",\"type\":\"string\"}]}");
      GenericRecord r3 = new GenericData.Record(bad);
      r3.put("id", "x");
      try {
        ser.serialize(avroTopic, r3);
        fail("avro-incompatible", "registered");
      } catch (Exception e) {
        String m = String.valueOf(e.getCause());
        check("avro-incompatible", m.contains("409"), m.length() > 160 ? m.substring(0, 160) : m);
      }
    } catch (Exception e) {
      fail("avro-evolve", e.toString());
    }
    // auto.register.schemas=false: a registered schema is found by lookup; an unknown one is an error.
    try (KafkaAvroSerializer ser = new KafkaAvroSerializer()) {
      ser.configure(conf(false, false), false);
      byte[] b = ser.serialize(avroTopic, rec);
      header("avro-lookup", b, 0);
      ok("avro-lookup", "found without registering");
      Schema other =
          new Schema.Parser()
              .parse("{\"type\":\"record\",\"name\":\"Nope\",\"fields\":[{\"name\":\"x\",\"type\":\"int\"}]}");
      GenericRecord r = new GenericData.Record(other);
      r.put("x", 1);
      try {
        ser.serialize(prefix + "avro-none", r);
        fail("avro-no-register", "serialized");
      } catch (Exception e) {
        String m = String.valueOf(e.getCause());
        check("avro-no-register", m.contains("40401") || m.contains("40403"), m);
      }
    } catch (Exception e) {
      fail("avro-lookup", e.toString());
    }
    // use.latest.version: the subject's latest schema is used whatever the record's.
    // Registered by the client first (as a deploy step would), then used by a serializer that registers nothing.
    String latestTopic = prefix + "avro-latest";
    try (KafkaAvroSerializer ser = new KafkaAvroSerializer()) {
      client.register(latestTopic + "-value", new AvroSchema(user));
      Map<String, Object> c = conf(false, true);
      ser.configure(c, false);
      byte[] b = ser.serialize(latestTopic, rec);
      int id = header("avro-use-latest", b, 0);
      int latest = client.getLatestSchemaMetadata(latestTopic + "-value").getId();
      check("avro-use-latest", id == latest, "the latest version's id");
    } catch (Exception e) {
      fail("avro-use-latest", e.toString());
    }

    // ---- Protobuf ----
    String proto =
        "syntax = \"proto3\";\npackage e2e;\nimport \"google/protobuf/timestamp.proto\";\n"
            + "message First { string a = 1; }\n"
            + "message Order { string id = 1; int64 cents = 2; google.protobuf.Timestamp at = 3;"
            + " message Line { int32 qty = 1; } repeated Line lines = 4; }\n";
    ProtobufSchema ps = new ProtobufSchema(proto);
    String pbTopic = prefix + "proto";
    try (KafkaProtobufSerializer<Message> ser = new KafkaProtobufSerializer<>();
        KafkaProtobufDeserializer<Message> de = new KafkaProtobufDeserializer<>()) {
      ser.configure(conf(true, false), false);
      de.configure(conf(true, false), false);
      Descriptor order = ps.toDescriptor("e2e.Order");
      Descriptor line = order.findNestedTypeByName("Line");
      DynamicMessage l = DynamicMessage.newBuilder(line).setField(line.findFieldByName("qty"), 3).build();
      DynamicMessage m =
          DynamicMessage.newBuilder(order)
              .setField(order.findFieldByName("id"), "o-1")
              .setField(order.findFieldByName("cents"), 1250L)
              .addRepeatedField(order.findFieldByName("lines"), l)
              .build();
      byte[] b = ser.serialize(pbTopic, m);
      // The second message of the file: indexes [1], written as count 1 then 1 (zig-zag: 02 02).
      header("protobuf-second-message", b, 2);
      check("protobuf-indexes", b[5] == 2 && b[6] == 2, String.format("%02x %02x", b[5], b[6]));
      Message back = de.deserialize(pbTopic, b);
      check("protobuf-round-trip", back.toString().equals(m.toString()), back.toString().replace("\n", " "));
      List<String> subjects = new ArrayList<>(client.getAllSubjects());
      // A well-known import is not registered as a reference subject: the serializer knows those types itself.
      check(
          "protobuf-well-known-import-not-registered",
          !subjects.contains("google/protobuf/timestamp.proto"),
          String.valueOf(subjects.stream().filter(s -> !s.startsWith(prefix)).toList()));
      Descriptor first = ps.toDescriptor("e2e.First");
      byte[] f = ser.serialize(pbTopic, DynamicMessage.newBuilder(first).setField(first.findFieldByName("a"), "x").build());
      // The first message: indexes [0], written as the single byte 0.
      header("protobuf-first-message", f, 1);
      check("protobuf-first-index", f[5] == 0, String.format("%02x", f[5]));
      DynamicMessage ln = DynamicMessage.newBuilder(line).setField(line.findFieldByName("qty"), 9).build();
      byte[] n = ser.serialize(pbTopic, ln);
      // Order.Line: indexes [1, 0], written as count 2 then 1 then 0 (04 02 00).
      header("protobuf-nested-message", n, 3);
      check("protobuf-nested-index", n[5] == 4 && n[6] == 2 && n[7] == 0, String.format("%02x %02x %02x", n[5], n[6], n[7]));
    } catch (Exception e) {
      fail("protobuf", e.toString());
      e.printStackTrace();
    }

    // ---- JSON Schema ----
    String js =
        "{\"$schema\":\"http://json-schema.org/draft-07/schema#\",\"title\":\"Item\",\"type\":\"object\","
            + "\"properties\":{\"sku\":{\"type\":\"string\"},\"qty\":{\"type\":\"integer\",\"minimum\":0}},"
            + "\"required\":[\"sku\"],\"additionalProperties\":false}";
    ObjectMapper om = new ObjectMapper();
    JsonNode item = om.readTree("{\"sku\":\"a-1\",\"qty\":2}");
    String jsTopic = prefix + "json";
    try (KafkaJsonSchemaSerializer<Object> ser = new KafkaJsonSchemaSerializer<>();
        KafkaJsonSchemaDeserializer<Object> de = new KafkaJsonSchemaDeserializer<>()) {
      Map<String, Object> c = conf(true, false);
      c.put("json.fail.invalid.schema", true);
      ser.configure(c, false);
      de.configure(c, false);
      byte[] b = ser.serialize(jsTopic, JsonSchemaUtils.envelope(new JsonSchema(js), item));
      header("json-auto-register", b, 0);
      Object back = de.deserialize(jsTopic, b);
      check("json-round-trip", om.valueToTree(back).equals(item), String.valueOf(back));
      try {
        ser.serialize(jsTopic, JsonSchemaUtils.envelope(new JsonSchema(js), om.readTree("{\"qty\":-1}")));
        fail("json-invalid-payload", "serialized");
      } catch (Exception e) {
        ok("json-invalid-payload", "refused by the serializer: " + e.getClass().getSimpleName());
      }
    } catch (Exception e) {
      fail("json", e.toString());
    }

    // ---- Through the broker ----
    Properties pp = new Properties();
    pp.put("bootstrap.servers", bootstrap);
    pp.put("key.serializer", StringSerializer.class.getName());
    pp.put("value.serializer", KafkaAvroSerializer.class.getName());
    pp.put("schema.registry.url", url);
    String topic = prefix + "wire";
    try (KafkaProducer<String, Object> p = new KafkaProducer<>(pp)) {
      p.send(new ProducerRecord<>(topic, "k", rec)).get();
      ok("broker-produce", "avro value produced");
    } catch (Exception e) {
      fail("broker-produce", e.toString());
    }
    Properties cp = new Properties();
    cp.put("bootstrap.servers", bootstrap);
    cp.put("group.id", prefix + "g");
    cp.put("auto.offset.reset", "earliest");
    cp.put("key.deserializer", StringDeserializer.class.getName());
    cp.put("value.deserializer", KafkaAvroDeserializer.class.getName());
    cp.put("schema.registry.url", url);
    try (KafkaConsumer<String, Object> c = new KafkaConsumer<>(cp)) {
      c.subscribe(List.of(topic));
      Object got = null;
      for (int i = 0; i < 30 && got == null; i++) {
        ConsumerRecords<String, Object> rs = c.poll(Duration.ofMillis(500));
        for (var r : rs) got = r.value();
      }
      check("broker-consume", got != null && got.toString().equals(rec.toString()), String.valueOf(got));
    } catch (Exception e) {
      fail("broker-consume", e.toString());
    }
    System.out.println(failed == 0 ? "all ok" : failed + " failed");
    System.exit(failed == 0 ? 0 : 1);
  }
}
