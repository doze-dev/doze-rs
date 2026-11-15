package main

import (
	"math/rand/v2"
	"time"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/display"
	"github.com/doze-dev/doze/internal/journal"
)

type eventIn struct {
	Time     int64  `json:"time,omitempty"`
	Kind     string `json:"kind,omitempty"`
	Instance string `json:"instance,omitempty"`
	Cause    string `json:"cause,omitempty"`
	By       string `json:"by,omitempty"`
	Took     int64  `json:"took,omitempty"`
	Detail   string `json:"detail,omitempty"`
}

func (e eventIn) event() journal.Event {
	return journal.Event{Time: time.Unix(0, e.Time).UTC(), Kind: e.Kind, Instance: e.Instance,
		Cause: journal.Cause{Kind: e.Cause, By: e.By}, Took: time.Duration(e.Took), Detail: e.Detail}
}

var (
	eventKinds = []string{"wake", "ready", "sleep", "crash", "restart", "give_up", "failed", "step", "pin", "odd", ""}
	causes     = []string{"connection", "dependency", "command", "restart", "idle", "shutdown", "watch", "", "odd"}
	details    = []string{"", "on", "off", "asked for", "needed", "exit status 1\nmore", "setup: migrate", "api", "\x1b[1mbold\x1b[0m"}
	eventTooks = []time.Duration{0, 1, 499_999, 500_000, 1_500_000, 999_999_999, time.Second, 1234 * time.Millisecond, 61 * time.Second, 2*time.Hour + 3*time.Millisecond}
)

func randomEvent(r *rand.Rand, instance string, t time.Time) eventIn {
	return eventIn{Time: t.UnixNano(), Kind: pick(r, eventKinds), Instance: instance, Cause: pick(r, causes),
		By: oneOf(r, "", "api", "doze.hcl"), Took: int64(pick(r, eventTooks)), Detail: pick(r, details)}
}

func journalCases(o *out, r *rand.Rand) {
	for range 800 {
		e := randomEvent(r, pick(r, names), now)
		ev := e.event()
		o.emit("event", e, map[string]any{"sentence": display.Event(ev), "cause_words": display.CauseWords(ev.Cause), "clean": display.CleanEvent(ev).Detail})
	}
	for range 400 {
		var events []eventIn
		t := now.Add(-time.Hour)
		for range r.IntN(12) {
			t = t.Add(time.Duration(r.IntN(5000)) * time.Millisecond)
			e := randomEvent(r, oneOf(r, "migrate", "migrate", "seed"), t)
			e.Kind = oneOf(r, "wake", "ready", "failed", "wake", "sleep")
			events = append(events, e)
		}
		var last *taskIn
		if r.IntN(2) == 0 {
			last = &taskIn{Result: oneOf(r, "done", "failed", ""), TookMS: pick(r, tooks), RanAt: at(when(r)), Why: pick(r, whys)}
		}
		var evs []journal.Event
		for _, e := range events {
			evs = append(evs, e.event())
		}
		var record *control.TaskView
		if last != nil {
			record = instIn{Task: last}.view().Task
		}
		var runs [][]any
		for _, run := range display.RunHistory(evs, "migrate", record) {
			runs = append(runs, []any{run.At.UnixNano(), run.Why, int64(run.Took), run.OK, run.Done})
		}
		o.emit("history", map[string]any{"events": list(events), "last": last}, list(runs))
	}
}
