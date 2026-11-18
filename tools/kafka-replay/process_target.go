package replay

import (
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"doze-rs/tools/kafka-replay/config"
)

// BinaryEnv names the doze-kafka binary the scenarios run against, as a process.
// The binary is any implementation of doze-kafka's command line that honours the
// test knobs below (the Rust edition reads them in every build; see
// docs/port/oracle.md).
const BinaryEnv = "CONFORMANCE_BIN"

// processTarget is a doze-kafka binary run as a child process over a data
// directory. A scenario configures the broker with a function over config.Config;
// the process is given that configuration as flags, and as the test knobs
// (environment variables) for the three things no flag covers.
type processTarget struct {
	bin     string
	dir     string
	addr    string
	args    []string
	env     []string
	control string
	sockDir string
	checks  time.Duration
	speed   float64
	cmd     *exec.Cmd
	done    chan struct{}
}

func startProcess(bin, dir string, mod func(*config.Config)) (*processTarget, error) {
	port, err := freePort()
	if err != nil {
		return nil, err
	}
	t := &processTarget{bin: bin, dir: dir, addr: fmt.Sprintf("127.0.0.1:%d", port)}
	cfg := config.Default()
	if mod != nil {
		mod(&cfg)
	}
	if err := t.configure(cfg); err != nil {
		return nil, err
	}
	return t, t.start()
}

// configure turns cfg into the process's arguments and environment. config.Config
// has only fields this passes, so a scenario can never be replayed against a
// different broker than it asks for.
func (t *processTarget) configure(cfg config.Config) error {
	data, meta := filepath.Join(t.dir, "data"), filepath.Join(t.dir, "meta.db")
	// A unix socket path is at most about 100 bytes (macOS), which a test's own
	// temporary directory can exceed: the socket gets a short directory of its own.
	sockDir, err := os.MkdirTemp("", "dk")
	if err != nil {
		return err
	}
	t.sockDir, t.control = sockDir, filepath.Join(sockDir, "c")
	t.args = []string{"--listen", t.addr, "--data-dir", data, "--meta-db", meta,
		"--group-initial-rebalance-delay", cfg.GroupInitialRebalanceDelay.String(),
		"--consumer-session-timeout", cfg.ConsumerSessionTimeout.String(),
		"--offsets-retention", cfg.OffsetsRetention.String(),
		"--acl-default", cfg.ACLDefault,
	}
	if len(cfg.SASLMechanisms) > 0 {
		t.args = append(t.args, "--sasl-mechanisms", strings.Join(cfg.SASLMechanisms, ","))
	}
	for _, user := range sortedKeys(cfg.Credentials) {
		t.args = append(t.args, "--sasl-user", user+":"+cfg.Credentials[user])
	}
	for _, p := range cfg.ACLSuperUsers {
		t.args = append(t.args, "--acl-super-user", p)
	}
	if cfg.Console {
		t.args = append(t.args, "--console", "--console-addr", "127.0.0.1:0")
	}
	t.checks = cfg.OffsetsRetentionCheckInterval
	t.env = append(os.Environ(), "DOZE_ZONE=off",
		"DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL="+cfg.ConsumerAssignmentInterval.String(),
		"DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL="+cfg.OffsetsRetentionCheckInterval.String(),
		"DOZE_KAFKA_TEST_CONTROL="+t.control)
	return nil
}

func (t *processTarget) start() error {
	os.Remove(t.control)
	log, err := os.OpenFile(filepath.Join(t.dir, "broker.log"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		return err
	}
	cmd := exec.Command(t.bin, t.args...)
	cmd.Env, cmd.Stdout, cmd.Stderr = t.env, log, log
	if err := cmd.Start(); err != nil {
		log.Close()
		return err
	}
	t.cmd, t.done = cmd, make(chan struct{})
	go func() { cmd.Wait(); log.Close(); close(t.done) }()
	return t.waitListening()
}

// waitListening returns once the broker accepts connections, or says why it did not.
func (t *processTarget) waitListening() error {
	deadline := time.Now().Add(30 * time.Second)
	for time.Now().Before(deadline) {
		select {
		case <-t.done:
			return fmt.Errorf("%s exited before it listened; see %s", t.bin, filepath.Join(t.dir, "broker.log"))
		default:
		}
		if c, err := net.DialTimeout("tcp", t.addr, time.Second); err == nil {
			c.Close()
			return nil
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.stop()
	return fmt.Errorf("%s did not listen on %s within 30s", t.bin, t.addr)
}

func (t *processTarget) Addr() string   { return t.addr }
func (t *processTarget) Speed() float64 { return t.speed }
func (t *processTarget) Restart() error { t.stop(); return t.start() }

// Close stops the broker and removes what it left for the test knobs.
func (t *processTarget) Close() {
	t.stop()
	os.RemoveAll(t.sockDir)
}

// stop stops the broker as an operator would (SIGTERM) and waits for it; one that
// does not stop within ten seconds is killed.
func (t *processTarget) stop() {
	if t.cmd == nil {
		return
	}
	t.cmd.Process.Signal(syscall.SIGTERM)
	select {
	case <-t.done:
	case <-time.After(10 * time.Second):
		t.cmd.Process.Kill()
		<-t.done
	}
	t.cmd = nil
}

// CheckOffsets applies the offsets retention now (Env.CheckOffsets), through the
// control socket of the test knobs, and returns once it has.
func (t *processTarget) CheckOffsets() {
	c, err := net.DialTimeout("unix", t.control, 5*time.Second)
	if err != nil {
		panic(fmt.Sprintf("conformance: the test control socket %s is not there (is the binary built with its test knobs?): %v", t.control, err))
	}
	defer c.Close()
	c.SetDeadline(time.Now().Add(30 * time.Second))
	fmt.Fprint(c, "check-offsets\n")
	buf := make([]byte, 3)
	if n, _ := c.Read(buf); string(buf[:n]) != "ok\n" {
		panic("conformance: the broker did not answer check-offsets")
	}
}
