/* A librdkafka transactional producer, for interop/run.sh: three transactions on a topic of three partitions, the same as the
 * Go producers write (ten records committed, ten aborted, five committed), with the records spread over the partitions so
 * that each transaction adds all three. Usage: txnprod <broker> <topic> <transactional.id> */
#include <librdkafka/rdkafka.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void die(const char *what, rd_kafka_error_t *err, const char *detail) {
    fprintf(stderr, "%s: %s\n", what, err ? rd_kafka_error_string(err) : detail);
    exit(1);
}

static void send_batch(rd_kafka_t *rk, const char *topic, const char *prefix, int n) {
    for (int i = 0; i < n; i++) {
        char value[32];
        snprintf(value, sizeof value, "%s%d", prefix, i);
        rd_kafka_resp_err_t e = rd_kafka_producev(
            rk, RD_KAFKA_V_TOPIC(topic), RD_KAFKA_V_PARTITION(i % 3),
            RD_KAFKA_V_VALUE(value, strlen(value)), RD_KAFKA_V_MSGFLAGS(RD_KAFKA_MSG_F_COPY),
            RD_KAFKA_V_END);
        if (e) die("produce", NULL, rd_kafka_err2str(e));
    }
}

static void transaction(rd_kafka_t *rk, const char *topic, const char *prefix, int n, int commit) {
    rd_kafka_error_t *err = rd_kafka_begin_transaction(rk);
    if (err) die("begin_transaction", err, NULL);
    send_batch(rk, topic, prefix, n);
    /* An abort purges what has not been sent, so the records are delivered first: the broker is to hold the aborted ones. */
    rd_kafka_flush(rk, 30000);
    err = commit ? rd_kafka_commit_transaction(rk, 30000) : rd_kafka_abort_transaction(rk, 30000);
    if (err) die(commit ? "commit_transaction" : "abort_transaction", err, NULL);
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: txnprod <broker> <topic> <transactional.id>\n");
        return 2;
    }
    char errstr[512];
    rd_kafka_conf_t *conf = rd_kafka_conf_new();
    if (rd_kafka_conf_set(conf, "bootstrap.servers", argv[1], errstr, sizeof errstr) ||
        rd_kafka_conf_set(conf, "transactional.id", argv[3], errstr, sizeof errstr) ||
        rd_kafka_conf_set(conf, "transaction.timeout.ms", "30000", errstr, sizeof errstr)) {
        fprintf(stderr, "config: %s\n", errstr);
        return 1;
    }
    rd_kafka_t *rk = rd_kafka_new(RD_KAFKA_PRODUCER, conf, errstr, sizeof errstr);
    if (!rk) {
        fprintf(stderr, "producer: %s\n", errstr);
        return 1;
    }
    rd_kafka_error_t *err = rd_kafka_init_transactions(rk, 30000);
    if (err) die("init_transactions", err, NULL);
    transaction(rk, argv[2], "c", 10, 1);
    transaction(rk, argv[2], "a", 10, 0);
    transaction(rk, argv[2], "d", 5, 1);
    rd_kafka_destroy(rk);
    puts("ok");
    return 0;
}
