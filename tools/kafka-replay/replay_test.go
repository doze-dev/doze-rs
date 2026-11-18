package replay

import (
	"flag"
	"os"
	"strings"
	"testing"
)

var (
	record = flag.Bool("record", false, "run every scenario against "+KafkaImage+" in Docker and rewrite recordings/")
	only   = flag.String("only", "", "with -record or on its own: only the scenario with this name")
)

const recordingsDir = "recordings"

// TestRecord is the recorder. It does nothing unless -record is given, and
// without Docker it says so and stops: a recording is what Apache Kafka
// answered, never something written by hand or taken from doze-kafka.
func TestRecord(t *testing.T) {
	if !*record {
		t.Skip("recorder: run with -record; needs Docker")
	}
	if !DockerAvailable() {
		t.Skip("Docker is not available: nothing was recorded")
	}
	for _, s := range Scenarios {
		if *only != "" && s.Name != *only {
			continue
		}
		if s.Frozen != "" {
			t.Run(s.Name, func(t *testing.T) { t.Skipf("frozen, not recorded again: %s", s.Frozen) })
			continue
		}
		t.Run(s.Name, func(t *testing.T) {
			target, err := StartKafkaFor(s)
			if err != nil {
				t.Fatal(err)
			}
			defer target.Close()
			steps, err := Run(s, target)
			if err != nil {
				t.Fatal(err)
			}
			if err := Save(recordingsDir, Recording{Scenario: s.Name, Kafka: KafkaImage, Steps: steps}); err != nil {
				t.Fatal(err)
			}
			t.Logf("recorded %d steps", len(steps))
		})
	}
}

// Every scenario has a recording, made against the image the recorder uses, with
// steps in it. (The Go repository's half of this test, that every Known
// difference cites a sentence of its docs/limitations.md, did not come over.)
func TestRecordingsAreCurrent(t *testing.T) {
	for _, s := range Scenarios {
		rec, err := Load(recordingsDir, s.Name)
		if err != nil {
			t.Errorf("%s: no recording: %v", s.Name, err)
			continue
		}
		if rec.Kafka != KafkaImage {
			t.Errorf("%s: recorded against %s, the recorder uses %s: record it again", s.Name, rec.Kafka, KafkaImage)
		}
		if len(rec.Steps) == 0 {
			t.Errorf("%s: an empty recording", s.Name)
		}
	}
}

// selected reports whether a scenario is in CONFORMANCE_ONLY: comma-separated name prefixes, unset for all. A binary
// that serves part of the protocol is checked against the scenarios of that part.
func selected(name string) bool {
	only := os.Getenv("CONFORMANCE_ONLY")
	if only == "" {
		return true
	}
	for _, prefix := range strings.Split(only, ",") {
		if prefix = strings.TrimSpace(prefix); prefix != "" && strings.HasPrefix(name, prefix) {
			return true
		}
	}
	return false
}

// TestReplay runs every scenario against the doze-kafka binary in CONFORMANCE_BIN,
// and compares each step with what Apache Kafka answered. A difference fails
// unless the scenario lists it as Known.
func TestReplay(t *testing.T) {
	if testing.Short() {
		t.Skip("boots a broker per scenario")
	}
	if *record {
		t.Skip("-record: recording, not replaying")
	}
	if os.Getenv(BinaryEnv) == "" {
		t.Fatalf("%s is not set: name the doze-kafka binary to replay against (see README.md)", BinaryEnv)
	}
	for _, s := range Scenarios {
		if *only != "" && s.Name != *only {
			continue
		}
		if !selected(s.Name) {
			continue
		}
		if s.DiskMiB > 0 || s.NoReplay != "" {
			continue // a volume of its own (Scenario.DiskMiB), or evidence only (Scenario.NoReplay).
		}
		t.Run(s.Name, func(t *testing.T) { replayOne(t, s) })
	}
}

// replayOne replays one scenario against the binary and compares it with its recording.
func replayOne(t *testing.T, s Scenario) {
	rec, err := Load(recordingsDir, s.Name)
	if err != nil {
		t.Fatalf("no recording: %v", err)
	}
	target, err := StartDozeFor(t.TempDir(), s)
	if err != nil {
		t.Fatal(err)
	}
	defer target.Close()
	steps, err := Run(s, target)
	if err != nil {
		t.Fatal(err)
	}
	diffs := Compare(rec, steps)
	unknown, _ := s.Unexplained(diffs)
	for _, u := range unknown {
		t.Errorf("differs from %s: %s", rec.Kafka, u)
	}
	// A Known entry that no longer differs is not reported: the list is the Go
	// edition's own, and an entry that matches nothing is this binary doing better
	// than that one.
}
