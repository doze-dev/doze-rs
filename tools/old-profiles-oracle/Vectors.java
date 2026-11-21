// Test vectors for crates/kafka-wire/src/batch/legacy (debt K2): message sets of format v0 and v1 as Kafka's own
// MemoryRecordsBuilder writes them, and v2 batches down-converted by Kafka's own RecordsUtil (MemoryRecords.downConvert).
//   cd interop && java -cp "$(bun java/fetch.ts 3.9)" ../tools/old-profiles-oracle/Vectors.java > ../crates/kafka-wire/tests/data/legacy-vectors.txt
import org.apache.kafka.common.compress.Compression;
import org.apache.kafka.common.record.*;
import org.apache.kafka.common.utils.Time;
import org.apache.kafka.common.header.Header;
import org.apache.kafka.common.header.internals.RecordHeader;
import java.nio.ByteBuffer;

public class Vectors {
    static String hex(ByteBuffer b) {
        StringBuilder s = new StringBuilder();
        ByteBuffer d = b.duplicate();
        while (d.hasRemaining()) s.append(String.format("%02x", d.get()));
        return s.toString();
    }

    static ByteBuffer legacy(byte magic, Compression c, TimestampType t, long base) {
        ByteBuffer buf = ByteBuffer.allocate(4096);
        MemoryRecordsBuilder b = MemoryRecords.builder(buf, magic, c, t, base, magic == 0 ? RecordBatch.NO_TIMESTAMP : 1_600_000_000_500L);
        b.appendWithOffset(base, 1_600_000_000_000L, "k0".getBytes(), "v0".getBytes());
        b.appendWithOffset(base + 1, 1_600_000_000_001L, null, "v1".getBytes());
        b.appendWithOffset(base + 2, 1_600_000_000_002L, "k2".getBytes(), null);
        return b.build().buffer();
    }

    static ByteBuffer v2(Compression c, TimestampType t, long base) {
        ByteBuffer buf = ByteBuffer.allocate(4096);
        MemoryRecordsBuilder b = MemoryRecords.builder(buf, (byte) 2, c, t, base, t == TimestampType.LOG_APPEND_TIME ? 1_600_000_000_900L : RecordBatch.NO_TIMESTAMP);
        b.append(new SimpleRecord(1_600_000_000_000L, "k0".getBytes(), "v0".getBytes(), new Header[] {new RecordHeader("h", "x".getBytes())}));
        b.append(new SimpleRecord(1_600_000_000_001L, null, "v1".getBytes()));
        b.append(new SimpleRecord(1_600_000_000_002L, "k2".getBytes(), null));
        return b.build().buffer();
    }

    public static void main(String[] a) {
        Compression[] cs = {Compression.NONE, Compression.gzip().build(), Compression.snappy().build(), Compression.lz4().build()};
        for (byte m = 0; m <= 1; m++)
            for (Compression c : cs)
                System.out.println("legacy m" + m + " " + c.type().name + " " + hex(legacy(m, c, TimestampType.CREATE_TIME, 0)));
        System.out.println("legacy m1 none append " + hex(legacy((byte) 1, Compression.NONE, TimestampType.LOG_APPEND_TIME, 0)));
        // Down-conversion of uncompressed v2 batches, as Kafka's own RecordsUtil does it (offsets from 10).
        for (TimestampType t : new TimestampType[] {TimestampType.CREATE_TIME, TimestampType.LOG_APPEND_TIME}) {
            MemoryRecords v = MemoryRecords.readableRecords(v2(Compression.NONE, t, 10));
            for (byte m = 0; m <= 1; m++)
                for (long first : new long[] {10, 11})
                    System.out.println("down " + t + " m" + m + " from " + first + " " + hex(((MemoryRecords) v.downConvert(m, first, Time.SYSTEM).records()).buffer()) + " in " + hex(v.buffer()));
        }
    }
}
