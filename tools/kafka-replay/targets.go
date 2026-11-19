package replay

import (
	"fmt"
	"net"
	"os"
	"strconv"
	"strings"

	"doze-rs/tools/kafka-replay/config"
)

// StartDozeFor starts the binary in BinaryEnv over dir with the configuration a
// scenario asks for: its SASL users, what Doze and Replay change, and the speed its
// waits run at.
func StartDozeFor(dir string, s Scenario) (Target, error) {
	bin := os.Getenv(BinaryEnv)
	if bin == "" {
		return nil, fmt.Errorf("%s is not set: name the doze-kafka binary to replay against", BinaryEnv)
	}
	t, err := startProcess(bin, dir, func(c *config.Config) {
		// What the recorded broker has (KAFKA_GROUP_INITIAL_REBALANCE_DELAY_MS=0 in
		// the recorder); a scenario that sets another sets both.
		c.GroupInitialRebalanceDelay = 0
		if len(s.Users) > 0 {
			c.SASLMechanisms = []string{config.SASLPlain}
			if len(s.Mechanisms) > 0 {
				c.SASLMechanisms = s.Mechanisms
			}
			c.Credentials = s.Users
		}
		if s.Doze != nil {
			s.Doze(c)
		}
		if s.Replay != nil {
			s.Replay(c)
		}
		c.KafkaVersion = s.KafkaVersion
	})
	if err != nil {
		return nil, err
	}
	t.speed = s.Speed
	return t, nil
}

// freePort is a free loopback port the OS picks, never one of the ports other
// local stacks on this machine keep for themselves (29181, 29192, 29194 and
// 39092 to 39699): one in those is let go and another asked for.
//
// With KAFKA_REPLAY_PORTS set to lo-hi (say 39900-39999) the port is the first free one in that range instead, for a
// machine where the free ports are shared out between people and agents.
func freePort() (int, error) {
	if r := os.Getenv("KAFKA_REPLAY_PORTS"); r != "" {
		return freePortIn(r)
	}
	var held []net.Listener
	defer func() {
		for _, ln := range held {
			ln.Close()
		}
	}()
	for range 64 {
		ln, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			return 0, err
		}
		port := ln.Addr().(*net.TCPAddr).Port
		if !reserved(port) {
			ln.Close()
			return port, nil
		}
		held = append(held, ln) // held open until we return, so the OS does not hand it out again.
	}
	return 0, fmt.Errorf("no free loopback port outside the reserved ranges")
}

// freePortIn is a free loopback port in lo-hi. Successive calls start after the last port handed out, so two
// brokers started one after the other (the Docker one publishes its port later) are not given the same one.
func freePortIn(r string) (int, error) {
	lo, hi, ok := strings.Cut(r, "-")
	from, err1 := strconv.Atoi(strings.TrimSpace(lo))
	to, err2 := strconv.Atoi(strings.TrimSpace(hi))
	if !ok || err1 != nil || err2 != nil || from > to {
		return 0, fmt.Errorf("KAFKA_REPLAY_PORTS=%q: want lo-hi", r)
	}
	for i := range to - from + 1 {
		port := from + (nextInRange+i)%(to-from+1)
		if reserved(port) {
			continue
		}
		ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", port))
		if err != nil {
			continue
		}
		ln.Close()
		nextInRange = (nextInRange + i + 1) % (to - from + 1)
		return port, nil
	}
	return 0, fmt.Errorf("no free loopback port in %s", r)
}

var nextInRange int

func reserved(port int) bool {
	switch {
	case port == 29181, port == 29192, port == 29194:
		return true
	case port >= 39092 && port <= 39699:
		return true
	}
	return false
}
