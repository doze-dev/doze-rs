package replay

import (
	"fmt"
	"net"
	"os"

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
func freePort() (int, error) {
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

func reserved(port int) bool {
	switch {
	case port == 29181, port == 29192, port == 29194:
		return true
	case port >= 39092 && port <= 39699:
		return true
	}
	return false
}
