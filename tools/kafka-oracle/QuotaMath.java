// Kafka's own quota arithmetic, for the vectors `crates/kafka-broker/src/quotas/meter/tests.rs` is held to. Run with the jars of
// the Apache Kafka 4.4.0 binary distribution on the class path:
//   java -cp '<kafka_2.13-4.4.0>/libs/*' tools/kafka-oracle/QuotaMath.java
import java.util.concurrent.TimeUnit;

import org.apache.kafka.common.metrics.MetricConfig;
import org.apache.kafka.common.metrics.Metrics;
import org.apache.kafka.common.metrics.Quota;
import org.apache.kafka.common.metrics.QuotaViolationException;
import org.apache.kafka.common.metrics.Sensor;
import org.apache.kafka.common.metrics.stats.Rate;
import org.apache.kafka.common.metrics.stats.TokenBucket;
import org.apache.kafka.common.utils.Time;
import org.apache.kafka.server.quota.QuotaUtils;

public class QuotaMath {
    static MetricConfig config(double bound) {
        return new MetricConfig().timeWindow(1, TimeUnit.SECONDS).samples(11).quota(Quota.upperBound(bound));
    }

    // A rate scenario: each step records a value at a time and prints the measured rate and the throttle (0 if none).
    static void rate(String name, double bound, long[][] steps) {
        Time time = Time.SYSTEM;
        Metrics metrics = new Metrics(new MetricConfig(), java.util.List.of(), time);
        Sensor sensor = metrics.sensor(name);
        sensor.add(metrics.metricName("rate", name), new Rate(), config(bound));
        System.out.println("rate " + name + " bound=" + bound);
        for (long[] s : steps) {
            long now = s[0];
            double value = s[1];
            long throttle = 0;
            double measured;
            try {
                sensor.record(value, now, true);
                measured = metrics.metric(metrics.metricName("rate", name)).measurable().measure(config(bound), now);
            } catch (QuotaViolationException e) {
                throttle = QuotaUtils.throttleTime(e, now);
                measured = e.value();
            }
            System.out.printf("  (%d, %.1f) -> measured %.6f throttle %d%n", now, value, measured, throttle);
        }
        metrics.close();
    }

    // A token-bucket scenario, strict: check first, then record (StrictControllerMutationQuota).
    static void bucket(String name, double bound, long[][] steps) {
        Time time = Time.SYSTEM;
        Metrics metrics = new Metrics(new MetricConfig(), java.util.List.of(), time);
        Sensor sensor = metrics.sensor(name);
        sensor.add(metrics.metricName("tokens", name), new TokenBucket(), config(bound));
        System.out.println("bucket " + name + " bound=" + bound);
        for (long[] s : steps) {
            long now = s[0];
            double value = s[1];
            String what;
            try {
                sensor.checkQuotas(now);
                sensor.record(value, now, false);
                what = "taken";
            } catch (QuotaViolationException e) {
                what = "refused throttle " + Math.round(-e.value() / e.bound() * 1000);
            }
            double tokens = metrics.metric(metrics.metricName("tokens", name)).measurable().measure(config(bound), now);
            System.out.printf("  (%d, %.1f) -> %s tokens %.6f%n", now, value, what, tokens);
        }
        metrics.close();
    }

    public static void main(String[] args) {
        long t = 1_700_000_000_000L;
        rate("burst", 1024, new long[][] {{t, 5000}, {t + 10, 5000}, {t + 500, 100}, {t + 3000, 1000}, {t + 20000, 100}});
        rate("steady", 1000, new long[][] {
            {t, 800}, {t + 1000, 800}, {t + 2000, 800}, {t + 3000, 800}, {t + 4000, 800}, {t + 5000, 800},
            {t + 6000, 800}, {t + 7000, 800}, {t + 8000, 800}, {t + 9000, 800}, {t + 10000, 800}, {t + 11000, 800},
            {t + 12000, 4000}, {t + 12500, 100}, {t + 30000, 1}});
        rate("windows", 100, new long[][] {
            {t, 50}, {t + 999, 50}, {t + 1000, 2000}, {t + 1001, -2000}, {t + 11000, 10}, {t + 11999, 10}, {t + 22000, 3000}});
        bucket("mutations", 10, new long[][] {
            {t, 50}, {t + 1, 70}, {t + 2, 1}, {t + 1000, 1}, {t + 3000, 1}, {t + 4000, 5}, {t + 4100, 200}, {t + 30000, 1}});
    }
}
