package main

// SASL mode (-sasl): each client library logs in with PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 to a broker started
// with `--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256 --sasl-user alice:...`, produces records, reads them
// back, and then tries a wrong password, which must be refused (and quickly). The cells are the product of the four
// libraries and the three mechanisms; none of the other scenarios runs in this mode, and none of them needs SASL.
//
// The same cells run over TLS (tls.go): "secure" then holds the CA to trust, every library connects with it, and a
// mechanism of "" means no SASL at all (the SSL listener).

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	lib "github.com/IBM/sarama"
	"github.com/segmentio/kafka-go"
	kgosasl "github.com/segmentio/kafka-go/sasl"
	kgoplain "github.com/segmentio/kafka-go/sasl/plain"
	kgoscram "github.com/segmentio/kafka-go/sasl/scram"
	"github.com/twmb/franz-go/pkg/kgo"
	"github.com/twmb/franz-go/pkg/kmsg"
	franzsasl "github.com/twmb/franz-go/pkg/sasl"
	franzplain "github.com/twmb/franz-go/pkg/sasl/plain"
	franzscram "github.com/twmb/franz-go/pkg/sasl/scram"
	"github.com/xdg-go/scram"
)

// The user the broker is started with (interop/run.sh -sasl); the password may be changed with SASL_PASSWORD.
const (
	saslUser = "alice"
	records  = 20
)

func saslPassword() string {
	if p := os.Getenv("SASL_PASSWORD"); p != "" {
		return p
	}
	return "alice-secret-pw"
}

// mechanisms are the names on the wire.
var mechanisms = []string{"PLAIN", "SCRAM-SHA-256", "SCRAM-SHA-512"}

// saslClient is one library's way to log in and move records.
type saslClient interface {
	name() string
	// produce writes the values "0".."records-1" to a topic of one partition, logged in as user/pass.
	produce(ctx context.Context, addr, mech, pass, topic string) error
	// consume reads the topic from its start until it has seen every value.
	consume(ctx context.Context, addr, mech, pass, topic string) error
	// login only logs in (and asks for metadata), returning the error if the broker refuses.
	login(ctx context.Context, addr, mech, pass string) error
}

// runSasl runs every cell and reports like the other scenarios; it returns whether all passed.
func runSasl(addr string) bool {
	clients := []saslClient{saramaSasl{}, franzSasl{}, kafkaGoSasl{}, kcatSasl{}}
	ok := true
	for _, mech := range mechanisms {
		for _, c := range clients {
			title := c.name() + "/" + mech
			start := time.Now()
			refused, err := cell(addr, c, mech)
			if err != nil {
				fmt.Printf("FAIL sasl %-32s %v\n", title, err)
				ok = false
				continue
			}
			fmt.Printf("ok   sasl %-32s %4.1fs  wrote and read %d records; a wrong password: %s\n", title, time.Since(start).Seconds(), records, refused)
		}
	}
	return ok
}

// cell runs one library and mechanism, and returns how the wrong password was refused (the library's words).
func cell(addr string, c saslClient, mech string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	topic := fmt.Sprintf("sasl-%s-%s-%d", c.name(), strings.ToLower(mech), time.Now().UnixNano()%1_000_000)
	if err := makeTopic(ctx, addr, topic, mech); err != nil {
		return "", fmt.Errorf("making the topic: %w", err)
	}
	if err := c.produce(ctx, addr, mech, saslPassword(), topic); err != nil {
		return "", fmt.Errorf("produce: %w", err)
	}
	if err := c.consume(ctx, addr, mech, saslPassword(), topic); err != nil {
		return "", fmt.Errorf("consume: %w", err)
	}
	if mech == "" {
		return untrustedCA(ctx, addr, c)
	}
	refusal, cancelRefusal := context.WithTimeout(ctx, 20*time.Second)
	defer cancelRefusal()
	began := time.Now()
	err := c.login(refusal, addr, mech, "not-the-password")
	if err == nil {
		return "", fmt.Errorf("a wrong password was accepted")
	}
	if refusal.Err() != nil {
		return "", fmt.Errorf("a wrong password was not refused within 20 s (%v)", err)
	}
	if time.Since(began) > 15*time.Second {
		return "", fmt.Errorf("a wrong password took %v to be refused", time.Since(began))
	}
	words := strings.Join(strings.Fields(err.Error()), " ")
	if !strings.Contains(strings.ToLower(words), "authentication") {
		return "", fmt.Errorf("a wrong password failed, but not as an authentication failure: %s", words)
	}
	if len(words) > 90 {
		words = words[:90] + "..."
	}
	return words, nil
}

// makeTopic creates the topic (one partition) with franz-go, logged in with mech as the user of the test.
func makeTopic(ctx context.Context, addr, topic, mech string) error {
	cl, err := kgo.NewClient(franzSasl{}.opts(addr, mech, saslPassword())...)
	if err != nil {
		return err
	}
	defer cl.Close()
	t := kmsg.NewCreateTopicsRequestTopic()
	t.Topic, t.NumPartitions, t.ReplicationFactor = topic, 1, 1
	req := kmsg.NewPtrCreateTopicsRequest()
	req.Topics = append(req.Topics, t)
	req.TimeoutMillis = 10000
	resp, err := req.RequestWith(ctx, cl)
	if err != nil {
		return err
	}
	if e := resp.Topics[0].ErrorCode; e != 0 {
		return fmt.Errorf("CreateTopics: error %d", e)
	}
	return nil
}

// seen checks that every value 0..records-1 was read.
func seen(got map[string]bool) error {
	for i := 0; i < records; i++ {
		if !got[fmt.Sprint(i)] {
			return fmt.Errorf("value %d was not read (%d distinct)", i, len(got))
		}
	}
	return nil
}

// ---- sarama

type saramaSasl struct{}

func (saramaSasl) name() string { return "sarama" }

// xdgSCRAM is the SCRAM client sarama asks for (its documentation's, over xdg-go/scram).
type xdgSCRAM struct {
	*scram.Client
	*scram.ClientConversation
	scram.HashGeneratorFcn
}

func (x *xdgSCRAM) Begin(user, password, authzID string) (err error) {
	x.Client, err = x.HashGeneratorFcn.NewClient(user, password, authzID)
	if err != nil {
		return err
	}
	x.ClientConversation = x.Client.NewConversation()
	return nil
}

func (x *xdgSCRAM) Step(challenge string) (string, error) {
	return x.ClientConversation.Step(challenge)
}
func (x *xdgSCRAM) Done() bool { return x.ClientConversation.Done() }

func (saramaSasl) config(mech, pass string) *lib.Config {
	cfg := lib.NewConfig()
	cfg.Version = lib.V3_6_0_0
	cfg.Producer.Return.Successes = true
	cfg.Producer.RequiredAcks = lib.WaitForAll
	if secure.cfg != nil {
		cfg.Net.TLS.Enable, cfg.Net.TLS.Config = true, secure.cfg
	}
	if mech == "" {
		return cfg
	}
	cfg.Net.SASL.Enable, cfg.Net.SASL.User, cfg.Net.SASL.Password = true, saslUser, pass
	switch mech {
	case "PLAIN":
		cfg.Net.SASL.Mechanism = lib.SASLTypePlaintext
	case "SCRAM-SHA-256":
		cfg.Net.SASL.Mechanism = lib.SASLTypeSCRAMSHA256
		cfg.Net.SASL.SCRAMClientGeneratorFunc = func() lib.SCRAMClient { return &xdgSCRAM{HashGeneratorFcn: scram.SHA256} }
	case "SCRAM-SHA-512":
		cfg.Net.SASL.Mechanism = lib.SASLTypeSCRAMSHA512
		cfg.Net.SASL.SCRAMClientGeneratorFunc = func() lib.SCRAMClient { return &xdgSCRAM{HashGeneratorFcn: scram.SHA512} }
	}
	return cfg
}

func (s saramaSasl) produce(_ context.Context, addr, mech, pass, topic string) error {
	p, err := lib.NewSyncProducer([]string{addr}, s.config(mech, pass))
	if err != nil {
		return err
	}
	defer p.Close()
	for i := 0; i < records; i++ {
		m := &lib.ProducerMessage{Topic: topic, Key: lib.StringEncoder(fmt.Sprint(i)), Value: lib.StringEncoder(fmt.Sprint(i))}
		if _, _, err := p.SendMessage(m); err != nil {
			return err
		}
	}
	return nil
}

func (s saramaSasl) consume(ctx context.Context, addr, mech, pass, topic string) error {
	cfg := s.config(mech, pass)
	cfg.Consumer.Offsets.Initial = lib.OffsetOldest
	c, err := lib.NewConsumer([]string{addr}, cfg)
	if err != nil {
		return err
	}
	defer c.Close()
	pc, err := c.ConsumePartition(topic, 0, lib.OffsetOldest)
	if err != nil {
		return err
	}
	defer pc.Close()
	got := map[string]bool{}
	for len(got) < records {
		select {
		case m := <-pc.Messages():
			got[string(m.Value)] = true
		case e := <-pc.Errors():
			return e
		case <-ctx.Done():
			return seen(got)
		}
	}
	return seen(got)
}

func (s saramaSasl) login(_ context.Context, addr, mech, pass string) error {
	cfg := s.config(mech, pass)
	cfg.Metadata.Retry.Max = 1
	cl, err := lib.NewClient([]string{addr}, cfg)
	if err != nil {
		return err
	}
	return cl.Close()
}

// ---- franz-go

type franzSasl struct{}

func (franzSasl) name() string { return "franz-go" }

// opts are the options of a client: the broker, TLS if the cells run over it, and the login unless mech is "".
func (f franzSasl) opts(addr, mech, pass string, more ...kgo.Opt) []kgo.Opt {
	opts := append([]kgo.Opt{kgo.SeedBrokers(addr)}, more...)
	if secure.cfg != nil {
		opts = append(opts, kgo.DialTLSConfig(secure.cfg))
	}
	if mech != "" {
		opts = append(opts, kgo.SASL(f.mechanism(mech, pass)))
	}
	return opts
}

func (franzSasl) mechanism(mech, pass string) franzsasl.Mechanism {
	switch mech {
	case "SCRAM-SHA-256":
		return franzscram.Auth{User: saslUser, Pass: pass}.AsSha256Mechanism()
	case "SCRAM-SHA-512":
		return franzscram.Auth{User: saslUser, Pass: pass}.AsSha512Mechanism()
	}
	return franzplain.Auth{User: saslUser, Pass: pass}.AsMechanism()
}

func (f franzSasl) produce(ctx context.Context, addr, mech, pass, topic string) error {
	cl, err := kgo.NewClient(f.opts(addr, mech, pass, kgo.DefaultProduceTopic(topic))...)
	if err != nil {
		return err
	}
	defer cl.Close()
	recs := make([]*kgo.Record, 0, records)
	for i := 0; i < records; i++ {
		recs = append(recs, &kgo.Record{Key: []byte(fmt.Sprint(i)), Value: []byte(fmt.Sprint(i))})
	}
	return cl.ProduceSync(ctx, recs...).FirstErr()
}

func (f franzSasl) consume(ctx context.Context, addr, mech, pass, topic string) error {
	cl, err := kgo.NewClient(f.opts(addr, mech, pass, kgo.ConsumeTopics(topic),
		kgo.ConsumeResetOffset(kgo.NewOffset().AtStart()))...)
	if err != nil {
		return err
	}
	defer cl.Close()
	got := map[string]bool{}
	for len(got) < records && ctx.Err() == nil {
		fetches := cl.PollFetches(ctx)
		if err := fetches.Err0(); err != nil && ctx.Err() == nil {
			return err
		}
		fetches.EachRecord(func(r *kgo.Record) { got[string(r.Value)] = true })
	}
	return seen(got)
}

func (f franzSasl) login(ctx context.Context, addr, mech, pass string) error {
	cl, err := kgo.NewClient(f.opts(addr, mech, pass)...)
	if err != nil {
		return err
	}
	defer cl.Close()
	return cl.Ping(ctx)
}

// ---- kafka-go

type kafkaGoSasl struct{}

func (kafkaGoSasl) name() string { return "kafka-go" }

func (kafkaGoSasl) mechanism(mech, pass string) (kgosasl.Mechanism, error) {
	switch mech {
	case "":
		return nil, nil
	case "SCRAM-SHA-256":
		return kgoscram.Mechanism(kgoscram.SHA256, saslUser, pass)
	case "SCRAM-SHA-512":
		return kgoscram.Mechanism(kgoscram.SHA512, saslUser, pass)
	}
	return kgoplain.Mechanism{Username: saslUser, Password: pass}, nil
}

func (k kafkaGoSasl) produce(ctx context.Context, addr, mech, pass, topic string) error {
	m, err := k.mechanism(mech, pass)
	if err != nil {
		return err
	}
	w := &kafka.Writer{
		Addr:         kafka.TCP(addr),
		Topic:        topic,
		Transport:    &kafka.Transport{SASL: m, TLS: secure.cfg},
		RequiredAcks: kafka.RequireAll,
		BatchTimeout: 10 * time.Millisecond,
	}
	defer w.Close()
	msgs := make([]kafka.Message, 0, records)
	for i := 0; i < records; i++ {
		msgs = append(msgs, kafka.Message{Key: []byte(fmt.Sprint(i)), Value: []byte(fmt.Sprint(i))})
	}
	return w.WriteMessages(ctx, msgs...)
}

func (k kafkaGoSasl) consume(ctx context.Context, addr, mech, pass, topic string) error {
	m, err := k.mechanism(mech, pass)
	if err != nil {
		return err
	}
	r := kafka.NewReader(kafka.ReaderConfig{
		Brokers:   []string{addr},
		Topic:     topic,
		Partition: 0,
		Dialer:    &kafka.Dialer{Timeout: 10 * time.Second, SASLMechanism: m, TLS: secure.cfg},
		MinBytes:  1,
		MaxBytes:  1 << 20,
		MaxWait:   500 * time.Millisecond,
	})
	defer r.Close()
	if err := r.SetOffset(kafka.FirstOffset); err != nil {
		return err
	}
	got := map[string]bool{}
	for len(got) < records {
		msg, err := r.ReadMessage(ctx)
		if err != nil {
			return fmt.Errorf("%w (read %d)", err, len(got))
		}
		got[string(msg.Value)] = true
	}
	return seen(got)
}

func (k kafkaGoSasl) login(ctx context.Context, addr, mech, pass string) error {
	m, err := k.mechanism(mech, pass)
	if err != nil {
		return err
	}
	d := &kafka.Dialer{Timeout: 10 * time.Second, SASLMechanism: m, TLS: secure.cfg}
	conn, err := d.DialContext(ctx, "tcp", addr)
	if err != nil {
		return err
	}
	defer conn.Close()
	_, err = conn.ReadPartitions()
	return err
}

// ---- librdkafka, through kcat

type kcatSasl struct{}

func (kcatSasl) name() string { return "librdkafka" }

// args are kcat's options for a login: the properties of librdkafka.
func (kcatSasl) args(addr, mech, pass string) []string {
	protocol := "SASL_PLAINTEXT"
	if secure.cfg != nil {
		protocol = "SASL_SSL"
		if mech == "" {
			protocol = "SSL"
		}
	}
	args := []string{"-b", addr, "-X", "security.protocol=" + protocol}
	if secure.caFile != "" {
		args = append(args, "-X", "ssl.ca.location="+secure.caFile)
	}
	if mech == "" {
		return args
	}
	return append(args, "-X", "sasl.mechanism="+mech, "-X", "sasl.username="+saslUser, "-X", "sasl.password="+pass)
}

func (k kcatSasl) produce(ctx context.Context, addr, mech, pass, topic string) error {
	var in strings.Builder
	for i := 0; i < records; i++ {
		fmt.Fprintf(&in, "%d:%d\n", i, i)
	}
	cmd := exec.CommandContext(ctx, "kcat", append(k.args(addr, mech, pass), "-P", "-t", topic, "-K:", "-p", "0")...)
	cmd.Stdin = strings.NewReader(in.String())
	if out, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("kcat -P: %v: %s", err, strings.TrimSpace(string(out)))
	}
	return nil
}

func (k kcatSasl) consume(ctx context.Context, addr, mech, pass, topic string) error {
	cmd := exec.CommandContext(ctx, "kcat", append(k.args(addr, mech, pass), "-C", "-t", topic, "-p", "0", "-o", "beginning", "-e", "-q", "-f", "%s\n")...)
	out, err := cmd.Output()
	if err != nil {
		return fmt.Errorf("kcat -C: %v", err)
	}
	got := map[string]bool{}
	for _, v := range strings.Fields(string(out)) {
		got[v] = true
	}
	return seen(got)
}

func (k kcatSasl) login(ctx context.Context, addr, mech, pass string) error {
	cmd := exec.CommandContext(ctx, "kcat", append(k.args(addr, mech, pass), "-L", "-m", "5")...)
	out, err := cmd.CombinedOutput()
	if err == nil {
		return nil
	}
	// librdkafka logs the broker's refusal; keep that line, and say plainly when there was none.
	for _, line := range strings.Split(string(out), "\n") {
		if at := strings.Index(line, "SASL authentication error"); at >= 0 {
			return fmt.Errorf("%s", line[at:])
		}
	}
	return fmt.Errorf("kcat -L: %v, with no authentication error: %s", err, strings.TrimSpace(string(out)))
}
