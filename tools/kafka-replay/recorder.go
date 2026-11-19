package replay

import (
	"fmt"
	"os/exec"
	"strings"
	"time"

	"github.com/twmb/franz-go/pkg/kmsg"
)

// --- Apache Kafka in Docker (the recorder) ---

// DockerAvailable reports whether `docker` can start containers here.
func DockerAvailable() bool {
	return exec.Command("docker", "info").Run() == nil
}

type dockerTarget struct {
	name       string
	port       int
	user, pass string // the login the readiness probe needs, when the listener is SASL.
}

// StartKafka runs a single-node KRaft apache/kafka (the compat matrix's image)
// published on a free loopback port, and waits until it answers. The settings
// are Kafka's own defaults except those a one-node cluster needs.
func StartKafka(image string) (Target, error) { return StartKafkaOn(image, 0) }

// StartKafkaOn is StartKafka with the data directory on a tmpfs of diskMiB
// mebibytes when that is above zero, so the disk can be filled.
func StartKafkaOn(image string, diskMiB int) (Target, error) {
	return startKafka(image, diskMiB, nil, "", "")
}

// StartKafkaFor runs the broker a scenario asks for: its disk, its settings, and
// the SASL users it serves.
func StartKafkaFor(s Scenario) (Target, error) {
	env := append([]string(nil), s.KafkaEnv...)
	probeUser, probePass := "", ""
	if len(s.Users) > 0 {
		jaas := fmt.Sprintf(`org.apache.kafka.common.security.plain.PlainLoginModule required username="%s" password="%s"`, s.Admin, s.Users[s.Admin])
		for _, u := range sortedKeys(s.Users) {
			jaas += fmt.Sprintf(` user_%s="%s"`, u, s.Users[u])
		}
		env = append(env,
			"KAFKA_LISTENER_SECURITY_PROTOCOL_MAP=CONTROLLER:PLAINTEXT,PLAINTEXT:SASL_PLAINTEXT",
			"KAFKA_SASL_ENABLED_MECHANISMS=PLAIN",
			"KAFKA_SASL_MECHANISM_INTER_BROKER_PROTOCOL=PLAIN",
			"KAFKA_INTER_BROKER_LISTENER_NAME=PLAINTEXT",
			"KAFKA_LISTENER_NAME_PLAINTEXT_PLAIN_SASL_JAAS_CONFIG="+jaas+";",
		)
		probeUser, probePass = s.Admin, s.Users[s.Admin]
		if scram := scramMechanisms(s.Mechanisms); len(scram) > 0 {
			// The listener's SCRAM users are not in a JAAS file: they are records in
			// the cluster's metadata, written below through the admin API once the
			// broker answers. (A triple underscore is the image's way to spell a
			// hyphen in a property name.)
			env = append(env, "KAFKA_SASL_ENABLED_MECHANISMS="+strings.Join(s.Mechanisms, ","))
			for _, m := range scram {
				env = append(env, "KAFKA_LISTENER_NAME_PLAINTEXT_"+strings.ReplaceAll(m, "-", "___")+
					"_SASL_JAAS_CONFIG=org.apache.kafka.common.security.scram.ScramLoginModule required;")
			}
		}
	}
	t, err := startKafka(s.RecordedImage(), s.DiskMiB, env, probeUser, probePass)
	if err != nil {
		return nil, err
	}
	if scram := scramMechanisms(s.Mechanisms); len(scram) > 0 {
		if err := createScramUsers(t.Addr(), s, scram); err != nil {
			t.Close()
			return nil, err
		}
	}
	return t, nil
}

func scramMechanisms(all []string) []string {
	var out []string
	for _, m := range all {
		if strings.HasPrefix(m, "SCRAM-") {
			out = append(out, m)
		}
	}
	return out
}

// createScramUsers gives every user of the scenario a SCRAM credential for each
// SCRAM mechanism, with the user's PLAIN password, ScramIterations and
// ScramSalt(user): AlterUserScramCredentials, sent by the admin over SASL/PLAIN.
// It is the scenario's fixture, not a step of it, and it waits until Kafka
// describes every credential, since the broker learns of them from the metadata
// log a moment after the request is answered.
func createScramUsers(addr string, s Scenario, mechanisms []string) error {
	w, err := dial(addr)
	if err != nil {
		return err
	}
	defer w.close()
	if err := w.saslPlain(s.Admin, s.Users[s.Admin]); err != nil {
		return fmt.Errorf("scram users: admin login: %v", err)
	}
	// One request per mechanism: Kafka refuses a user named twice in one
	// (DUPLICATE_RESOURCE).
	for _, m := range mechanisms {
		req := kmsg.NewPtrAlterUserSCRAMCredentialsRequest()
		for _, u := range sortedKeys(s.Users) {
			up := kmsg.NewAlterUserSCRAMCredentialsRequestUpsertion()
			up.Name, up.Mechanism, up.Iterations = u, scramMechanismCode(m), ScramIterations
			up.Salt = ScramSalt(u)
			up.SaltedPassword = scramSalted(m, s.Users[u], up.Salt, ScramIterations)
			req.Upsertions = append(req.Upsertions, up)
		}
		resp, err := w.roundTrip(req, 0)
		if err != nil {
			return fmt.Errorf("scram users: %v", err)
		}
		for _, r := range resp.(*kmsg.AlterUserSCRAMCredentialsResponse).Results {
			if r.ErrorCode != 0 {
				return fmt.Errorf("scram users: %s: error %d", r.User, r.ErrorCode)
			}
		}
	}
	for deadline := time.Now().Add(30 * time.Second); ; time.Sleep(100 * time.Millisecond) {
		resp, err := w.roundTrip(kmsg.NewPtrDescribeUserSCRAMCredentialsRequest(), 0)
		if err != nil {
			return fmt.Errorf("scram users: describe: %v", err)
		}
		n := 0
		for _, r := range resp.(*kmsg.DescribeUserSCRAMCredentialsResponse).Results {
			n += len(r.CredentialInfos)
		}
		if n == len(s.Users)*len(mechanisms) {
			return nil
		}
		if time.Now().After(deadline) {
			return fmt.Errorf("scram users: Kafka describes %d credentials, want %d", n, len(s.Users)*len(mechanisms))
		}
	}
}

func startKafka(image string, diskMiB int, extra []string, probeUser, probePass string, mounts ...string) (Target, error) {
	port, err := freePort()
	if err != nil {
		return nil, err
	}
	name := fmt.Sprintf("doze-conformance-%d", time.Now().UnixNano())
	args := []string{"run", "-d", "--name", name, "-p", fmt.Sprintf("127.0.0.1:%d:9092", port),
		"-e", "KAFKA_NODE_ID=1",
		"-e", "KAFKA_PROCESS_ROLES=broker,controller",
		"-e", "KAFKA_LISTENERS=PLAINTEXT://:9092,CONTROLLER://:9093",
		"-e", fmt.Sprintf("KAFKA_ADVERTISED_LISTENERS=PLAINTEXT://127.0.0.1:%d", port),
		"-e", "KAFKA_CONTROLLER_LISTENER_NAMES=CONTROLLER",
		"-e", "KAFKA_LISTENER_SECURITY_PROTOCOL_MAP=CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT",
		"-e", "KAFKA_CONTROLLER_QUORUM_VOTERS=1@localhost:9093",
		"-e", "KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR=1",
		"-e", "KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR=1",
		"-e", "KAFKA_TRANSACTION_STATE_LOG_MIN_ISR=1",
		"-e", "KAFKA_GROUP_INITIAL_REBALANCE_DELAY_MS=0",
	}
	// A later -e of the same name wins, so a scenario's settings replace the defaults.
	for _, kv := range extra {
		// A scenario that adds a listener of its own has to have it advertised too,
		// and the advertised list carries the port chosen above.
		if strings.HasPrefix(kv, "KAFKA_LISTENERS=") && strings.Contains(kv, "INTERNAL://") {
			args = append(args, "-e", fmt.Sprintf("KAFKA_ADVERTISED_LISTENERS=PLAINTEXT://127.0.0.1:%d,INTERNAL://localhost:9094", port))
		}
		args = append(args, "-e", kv)
	}
	for _, m := range mounts {
		args = append(args, "-v", m)
	}
	args = append(args, image)
	if diskMiB > 0 {
		args = append([]string{"run", "-d", "--tmpfs", fmt.Sprintf("/var/lib/kafka/data:size=%dm,mode=1777", diskMiB),
			"-e", "KAFKA_LOG_DIRS=/var/lib/kafka/data"}, args[2:]...)
	}
	if out, err := exec.Command("docker", args...).CombinedOutput(); err != nil {
		return nil, fmt.Errorf("docker run: %v: %s", err, strings.TrimSpace(string(out)))
	}
	t := &dockerTarget{name: name, port: port, user: probeUser, pass: probePass}
	if err := t.wait(); err != nil {
		logs, _ := exec.Command("docker", "logs", "--tail", "30", name).CombinedOutput()
		t.Close()
		return nil, fmt.Errorf("%v\n%s", err, logs)
	}
	return t, nil
}

func (t *dockerTarget) Addr() string { return fmt.Sprintf("127.0.0.1:%d", t.port) }

// wait polls with a real request until the broker answers it: the port opens
// well before KRaft has elected itself.
func (t *dockerTarget) wait() error {
	deadline := time.Now().Add(90 * time.Second)
	for time.Now().Before(deadline) {
		if ready(t.Addr(), t.user, t.pass) {
			return nil
		}
		time.Sleep(500 * time.Millisecond)
	}
	return fmt.Errorf("kafka in %s did not become ready", t.name)
}

func (t *dockerTarget) Restart() error {
	if out, err := exec.Command("docker", "restart", "-t", "30", t.name).CombinedOutput(); err != nil {
		return fmt.Errorf("docker restart: %v: %s", err, out)
	}
	return t.wait()
}

// Exec runs a command in the broker's container (Execer): a script fills the
// volume the log directory is on from beside the broker.
func (t *dockerTarget) Exec(args ...string) (string, error) {
	out, err := exec.Command("docker", append([]string{"exec", "-u", "root", t.name}, args...)...).CombinedOutput()
	return string(out), err
}

func (t *dockerTarget) Close() { exec.Command("docker", "rm", "-f", t.name).Run() }

// ready sends one Metadata request for no topics and wants a broker back.
func ready(addr, user, pass string) bool {
	w, err := dialOnce(addr)
	if err != nil {
		return false
	}
	defer w.close()
	if user != "" && w.saslPlain(user, pass) != nil {
		return false
	}
	return probe(w)
}
