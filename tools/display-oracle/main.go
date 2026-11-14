// The Go doze's own words, for the Rust crate doze-display to match byte for byte (docs/port/app/PLAN.md, step 0.4):
// every exported function of internal/display, run over a generated set of inputs, written as one JSON object per
// line. The Rust test crates/doze-display/tests/oracle reads the inputs back, says the same things, and compares.
//
//	go run . > ../../crates/doze-display/tests/fixtures/display.jsonl
//
// The module path sits under github.com/doze-dev/doze so that it may import that module's internal packages; the
// replace points at a checkout of doze, which must be at origin/main when the golden is written. Generate on an
// arm64 machine or an amd64 one alike: the inputs avoid the float-to-integer conversions Go leaves to the platform.
// Floats are written as their IEEE bits, so nothing is lost to decimal.
package main

import (
	"bufio"
	"encoding/json"
	"math/rand/v2"
	"os"
	"time"
)

// home is where ~ stands for every case (Tilde reads $HOME; the Rust side is handed the same).
const home = "/home/dev"

// now is the clock every case is read at: 2026-10-11 14:04:40 UTC, as words_test.go has it.
var now = time.Date(2026, 10, 11, 14, 4, 40, 0, time.UTC)

type out struct {
	w *bufio.Writer
	n int
}

func (o *out) emit(kind string, in, got any) {
	b, err := json.Marshal(map[string]any{"case": kind, "in": in, "out": got})
	if err != nil {
		panic(err)
	}
	o.w.Write(b)
	o.w.WriteByte('\n')
	o.n++
}

func main() {
	if err := os.Setenv("HOME", home); err != nil {
		panic(err)
	}
	o := &out{w: bufio.NewWriter(os.Stdout)}
	r := rand.New(rand.NewPCG(4, 11))
	for range 200 {
		snapCase(o, r)
	}
	stringCases(o, r)
	numberCases(o, r)
	seriesCases(o, r)
	journalCases(o, r)
	for range 100 {
		consoleCase(o, r)
	}
	if err := o.w.Flush(); err != nil {
		panic(err)
	}
	os.Stderr.WriteString("display-oracle: wrote " + itoa(o.n) + " cases\n")
}

func itoa(n int) string {
	b, _ := json.Marshal(n)
	return string(b)
}

func pick[T any](r *rand.Rand, xs []T) T { return xs[r.IntN(len(xs))] }

// at is a time as nanoseconds since the Unix epoch, or nil for the zero time.
func at(t time.Time) *int64 {
	if t.IsZero() {
		return nil
	}
	n := t.UnixNano()
	return &n
}

func fromAt(n *int64) time.Time {
	if n == nil {
		return time.Time{}
	}
	return time.Unix(0, *n).UTC()
}

// offsets from now that sit on the edges of the units the words are written in.
var offsets = []time.Duration{
	-3 * time.Second, -500 * time.Millisecond, 0, 1, 499 * time.Millisecond, 500 * time.Millisecond,
	999 * time.Millisecond, time.Second, 1500 * time.Millisecond, 29500 * time.Millisecond, 59 * time.Second,
	59500 * time.Millisecond, 61 * time.Second, 299 * time.Second, 301 * time.Second, 3599 * time.Second, time.Hour,
	47 * time.Hour, 48 * time.Hour, 49 * time.Hour, 72*time.Hour + 123*time.Millisecond, 400 * 24 * time.Hour,
}

// when is a time near now (before or after), or the zero time.
func when(r *rand.Rand) time.Time {
	if r.IntN(6) == 0 {
		return time.Time{}
	}
	d := pick(r, offsets)
	if r.IntN(4) == 0 {
		return now.Add(d)
	}
	return now.Add(-d)
}
