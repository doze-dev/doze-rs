package fixture

import (
	"time"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/control/fakedaemon"
	"github.com/doze-dev/doze/internal/journal"
)

// States is a stack with every state an instance can be in, at once: one
// instance each, so a screen is checked against the whole vocabulary in one
// picture. awake, pinned, held by a dependant and connections, dozing with its
// countdown, waking (woken by, waiting on), stuck waking, asleep, crashing,
// failed after giving up, not ready in time, off, paused, and a task not run,
// running, done and failed. Its modules' consoles are live, asleep (kept),
// in error and not yet read.
func States(now time.Time) *fakedaemon.Fake {
	ago := func(d time.Duration) time.Time { return now.Add(-d) }
	f := &fakedaemon.Fake{Hold: true}
	f.Snap = control.Snapshot{Stack: "states", Dir: "~/code/states", IdleTimeout: 5 * time.Minute, Instances: []control.InstanceView{
		{Name: "api", Kind: "app", Engine: "process", State: "active", Run: "go run ./cmd/api", RAM: 84 << 20, CPU: 2.1, PID: 48211, Conns: 3,
			StartedAt: ago(14 * time.Minute), Domain: "api.states.doze", Bind: "127.0.0.1:8080", Endpoint: "127.0.0.1:8080",
			Needs: []string{"db", "cache"}, Holders: []control.HolderView{{Kind: "connection", Count: 3}}},
		{Name: "web", Kind: "app", Engine: "process", State: "active", Run: "pnpm dev", RAM: 212 << 20, CPU: 0.4, PID: 48230,
			StartedAt: ago(31 * time.Minute), KeepAwake: true, Holders: []control.HolderView{{Kind: "pin"}},
			Domain: "web.states.doze", Bind: "127.0.0.1:3000", Endpoint: "127.0.0.1:3000"},
		{Name: "db", Kind: "module", Engine: "postgres", Version: "17", State: "active", RAM: 96 << 20, CPU: 0.8, PID: 612, Conns: 4,
			StartedAt: ago(15 * time.Minute), URL: "postgres://app@db.states.doze:5432/app", Domain: "db.states.doze",
			Endpoint: "127.0.0.1:5432", Bind: "127.0.0.1:5432", EnvVar: "DATABASE_URL", HasConsole: true, ConsoleFront: "overview",
			Holders: []control.HolderView{{Kind: "dependant", Name: "api"}, {Kind: "connection", Count: 4}}},
		{Name: "cache", Kind: "module", Engine: "valkey", Version: "9", State: "idle", RAM: 11 << 20, CPU: 0.1, PID: 701,
			StartedAt: ago(12 * time.Minute), IdleSince: ago(108 * time.Second), HasConsole: true,
			Domain: "cache.states.doze", Endpoint: "127.0.0.1:6379", Bind: "127.0.0.1:6379"},
		{Name: "events", Kind: "module", Engine: "kafka", Version: "4", State: "booting", StartedAt: ago(2 * time.Second),
			Holders: []control.HolderView{{Kind: "dependant", Name: "worker"}}},
		{Name: "worker", Kind: "app", Engine: "process", State: "booting", Run: "go run ./cmd/worker", Needs: []string{"events"}},
		{Name: "indexer", Kind: "service", Engine: "process", State: "booting", Run: "bin/indexer --watch", StartedAt: ago(7 * time.Minute)},
		{Name: "search", Kind: "service", Engine: "process", State: "reaped", Run: "docker run --rm -p 7700:7700 getmeili/meilisearch"},
		{Name: "flows", Kind: "module", Engine: "temporal", Version: "1.1", State: "reaped", HasConsole: true, ConsoleFront: "web",
			WebURL: "http://127.0.0.1:8233", Domain: "flows.states.doze", Endpoint: "127.0.0.1:7233"},
		{Name: "store", Kind: "module", Engine: "kvrocks", Version: "2.11", State: "active", RAM: 40 << 20, CPU: 0.3, PID: 760, Conns: 1,
			StartedAt: ago(10 * time.Minute), HasConsole: true, Holders: []control.HolderView{{Kind: "connection", Count: 1}},
			Domain: "store.states.doze", Endpoint: "127.0.0.1:6666", Bind: "127.0.0.1:6666"},
		{Name: "files", Kind: "module", Engine: "aws", State: "active", RAM: 64 << 20, CPU: 0.6, PID: 752, Conns: 1, HasConsole: true,
			StartedAt: ago(9 * time.Minute), Holders: []control.HolderView{{Kind: "connection", Count: 1}},
			Domain: "files.states.doze", Endpoint: "127.0.0.1:4566", Bind: "127.0.0.1:4566"},
		{Name: "gateway", Kind: "app", Engine: "process", State: "crashing", Run: "go run ./cmd/gateway",
			LastError:    "panic: STRIPE_KEY is required\n  main.loadConfig  cmd/gateway/config.go:41",
			RestartCount: 3, RestartMax: 5, NextRestart: now.Add(6 * time.Second)},
		{Name: "billing", Kind: "app", Engine: "process", State: "failed", Run: "go run ./cmd/billing",
			LastError: "exit status 2; gave up after 5 restarts"},
		{Name: "notifier", Kind: "service", Engine: "process", State: "failed", Run: "bin/notifier",
			LastError: "did not become ready within 30s"},
		{Name: "mailer", Kind: "service", Engine: "process", State: "reaped", Run: "mailpit --smtp 127.0.0.1:$PORT", Disabled: true},
		{Name: "legacy", Kind: "app", Engine: "process", State: "reaped", Run: "php -S 127.0.0.1:$PORT", Disabled: true, Paused: true},
		{Name: "migrate", Kind: "task", Engine: "process", State: "reaped", Needs: []string{"db"}},
		{Name: "seed", Kind: "task", Engine: "process", State: "reaped",
			Task: &control.TaskView{Result: "done", TookMS: 1200, RanAt: ago(2 * time.Minute), Why: "doze up"}},
		{Name: "backfill", Kind: "task", Engine: "process", State: "reaped",
			Task: &control.TaskView{Result: "failed", ExitCode: 3, TookMS: 412, RanAt: ago(9 * time.Minute), Why: "you ran it"}},
		{Name: "codegen", Kind: "task", Engine: "process", State: "booting", StartedAt: ago(4 * time.Second)},
	}}

	for i := 0; i < 240; i++ {
		f.Frames = append(f.Frames, control.Frame{Topic: control.TopicStats, Stats: []control.StatView{
			{Name: "api", PID: 48211, RAM: climb(i, 70, 84, 55), CPU: wave(i, 2.2, 1.6, 17, 41, 9), Conns: 3},
			{Name: "web", PID: 48230, RAM: climb(i, 205, 214, 70), CPU: wave(i, 0.6, 0.4, 13, 0, 0)},
			{Name: "db", PID: 612, RAM: climb(i, 88, 96, 80), CPU: wave(i, 1.2, 0.9, 23, 59, 6), Conns: 4},
			{Name: "cache", PID: 701, RAM: climb(i, 10, 11, 120), CPU: wave(i, 0.1, 0.1, 19, 0, 0)},
			{Name: "store", PID: 760, RAM: climb(i, 36, 40, 90), CPU: wave(i, 0.3, 0.2, 29, 0, 0), Conns: 1},
			{Name: "files", PID: 752, RAM: climb(i, 60, 64, 100), CPU: wave(i, 0.6, 0.4, 37, 0, 0), Conns: 1},
		}})
	}

	ev := func(d time.Duration, kind, inst string, c journal.Cause, took time.Duration) {
		e := journal.Event{Time: ago(d), Kind: kind, Instance: inst, Cause: c, Took: took}
		f.Frames = append(f.Frames, control.Frame{Topic: control.TopicJournal, Event: &e})
	}
	ev(15*time.Minute, journal.Wake, "db", journal.Cause{Kind: journal.ByDependency, By: "api"}, 0)
	ev(15*time.Minute-180*time.Millisecond, journal.Ready, "db", journal.Cause{}, 180*time.Millisecond)
	ev(12*time.Minute, journal.Wake, "cache", journal.Cause{Kind: journal.ByDependency, By: "api"}, 0)
	ev(7*time.Minute, journal.Wake, "indexer", journal.Cause{Kind: journal.ByCommand}, 0)
	ev(4*time.Minute, journal.Crash, "gateway", journal.Cause{}, 0)
	ev(3*time.Minute, journal.GiveUp, "billing", journal.Cause{}, 0)
	ev(2*time.Minute, journal.Failed, "notifier", journal.Cause{}, 0)
	ev(2*time.Second, journal.Wake, "events", journal.Cause{Kind: journal.ByDependency, By: "worker"}, 0)

	pgSpec, pgActions := PostgresConsole()
	f.Frames = append(f.Frames, consoleFrames("db", pgSpec, pgActions, map[string][]map[string]string{
		"activity": {{"active": "2", "idle": "2", "waiting": "1", "of": "100", "longest": "4.2 s"}},
		"sessions": {
			{"pid": "4412", "user": "app", "database": "app", "app": "api", "for": "4.2 s", "state": "waiting", "query": "UPDATE orders SET status = $1 WHERE id = $2"},
			{"pid": "4410", "user": "app", "database": "app", "app": "api", "for": "1.1 s", "state": "active", "query": "SELECT * FROM orders WHERE customer_id = $1"},
		},
		"databases": {{"database": "app", "size": "38 MB", "connections": "4", "owner": "postgres"}},
	}, nil)...)
	// files: the module's console answered with an error.
	aSpec, aActions := AwsConsole()
	files := consoleFrames("files", aSpec, aActions, nil, nil)
	files = append(files, control.Frame{Topic: control.TopicConsole, Console: &control.ConsoleFrame{Instance: "files", Error: "the module did not answer: context deadline exceeded"}})
	f.Frames = append(f.Frames, files...)
	// flows: asleep; what its console last showed is kept.
	tSpec := control.ConsoleSpec{Front: "web", WebURL: "http://127.0.0.1:8233", Panels: []control.ConsolePanel{
		{ID: "namespaces", Title: "Namespaces", Kind: "table", Source: "namespace", Columns: []control.ConsoleColumn{
			{Key: "name", Title: "Namespace"}, {Key: "status", Title: "State", Role: "state"}, {Key: "retention", Title: "Retention"},
			{Key: "running", Title: "Running", Align: "right", Role: "count"}}}},
		Extend: []control.ConsoleExtension{{After: "vitals", Node: control.ConsoleNode{Kind: "table", Panel: "namespaces", Title: "Namespaces", Weight: 1}}}}
	flows := consoleFrames("flows", tSpec, nil, map[string][]map[string]string{
		"namespaces": {{"name": "default", "status": "registered", "retention": "7 days", "running": "3"}},
	}, nil)
	if c := flows[0].Console; c != nil {
		c.State = "reaped"
	}
	f.Frames = append(f.Frames, flows...)
	// cache and store: consoles not read yet (no frames).

	line := func(inst string, d time.Duration, s string) {
		l := control.LogFrame{Instance: inst, Line: s, Time: ago(d), Run: 1}
		f.Frames = append(f.Frames, control.Frame{Topic: control.TopicLogs, Log: &l})
		if f.Lines == nil {
			f.Lines = map[string][]control.LogFrame{}
		}
		f.Lines[inst] = append(f.Lines[inst], l)
	}
	line("api", 14*time.Minute, "INFO listening addr=:8080")
	line("api", time.Minute, "INFO GET /orders 200 6ms")
	line("gateway", 4*time.Second, "panic: STRIPE_KEY is required")
	line("billing", 3*time.Minute, "exit status 2")
	line("backfill", 9*time.Minute, "ERROR: relation \"orders_archive\" does not exist")
	line("seed", 2*time.Minute, "seeded 40 products")
	return f
}
