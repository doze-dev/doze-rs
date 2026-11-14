package main

import (
	"math/rand/v2"
	"time"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/display"
)

type taskIn struct {
	Result string `json:"result,omitempty"`
	Exit   int    `json:"exit,omitempty"`
	TookMS int64  `json:"took_ms,omitempty"`
	RanAt  *int64 `json:"ran_at,omitempty"`
	Why    string `json:"why,omitempty"`
}

type holderIn struct {
	Kind  string `json:"kind,omitempty"`
	Name  string `json:"name,omitempty"`
	Count int    `json:"count,omitempty"`
}

// instIn is an InstanceView as the Rust test reads it: times as nanoseconds, or null for the zero time.
type instIn struct {
	Name         string     `json:"name,omitempty"`
	Kind         string     `json:"kind,omitempty"`
	Run          string     `json:"run,omitempty"`
	Engine       string     `json:"engine,omitempty"`
	State        string     `json:"state,omitempty"`
	Version      string     `json:"version,omitempty"`
	Task         *taskIn    `json:"task,omitempty"`
	Needs        []string   `json:"needs,omitempty"`
	Holders      []holderIn `json:"holders,omitempty"`
	StartedAt    *int64     `json:"started_at,omitempty"`
	IdleSince    *int64     `json:"idle_since,omitempty"`
	NextRestart  *int64     `json:"next_restart,omitempty"`
	IdleAfter    int64      `json:"idle_after,omitempty"`
	AlwaysOn     bool       `json:"always_on,omitempty"`
	NameOnly     bool       `json:"name_only,omitempty"`
	HasConsole   bool       `json:"has_console,omitempty"`
	Tainted      bool       `json:"tainted,omitempty"`
	Disabled     bool       `json:"disabled,omitempty"`
	Paused       bool       `json:"paused,omitempty"`
	KeepAwake    bool       `json:"keep_awake,omitempty"`
	RAM          int64      `json:"ram,omitempty"`
	Endpoint     string     `json:"endpoint,omitempty"`
	Bind         string     `json:"bind,omitempty"`
	Domain       string     `json:"domain,omitempty"`
	Console      string     `json:"console,omitempty"`
	ConsoleFront string     `json:"console_front,omitempty"`
	WebURL       string     `json:"web_url,omitempty"`
	NameHeld     string     `json:"name_held,omitempty"`
	URL          string     `json:"url,omitempty"`
	Resource     string     `json:"resource,omitempty"`
	DataDir      string     `json:"data_dir,omitempty"`
	LastError    string     `json:"last_error,omitempty"`
	RestartCount int        `json:"restart_count,omitempty"`
	RestartMax   int        `json:"restart_max,omitempty"`
}

func (i instIn) view() control.InstanceView {
	v := control.InstanceView{
		Name: i.Name, Kind: i.Kind, Run: i.Run, Engine: i.Engine, State: i.State, Version: i.Version, Needs: i.Needs,
		StartedAt: fromAt(i.StartedAt), IdleSince: fromAt(i.IdleSince), NextRestart: fromAt(i.NextRestart),
		IdleAfter: time.Duration(i.IdleAfter), AlwaysOn: i.AlwaysOn, NameOnly: i.NameOnly, HasConsole: i.HasConsole,
		Tainted: i.Tainted, Disabled: i.Disabled, Paused: i.Paused, KeepAwake: i.KeepAwake, RAM: i.RAM,
		Endpoint: i.Endpoint, Bind: i.Bind, Domain: i.Domain, Console: i.Console, ConsoleFront: i.ConsoleFront,
		WebURL: i.WebURL, NameHeld: i.NameHeld, URL: i.URL, Resource: i.Resource, DataDir: i.DataDir,
		LastError: i.LastError, RestartCount: i.RestartCount, RestartMax: i.RestartMax,
	}
	for _, h := range i.Holders {
		v.Holders = append(v.Holders, control.HolderView{Kind: h.Kind, Name: h.Name, Count: h.Count})
	}
	if i.Task != nil {
		v.Task = &control.TaskView{Result: i.Task.Result, ExitCode: i.Task.Exit, TookMS: i.Task.TookMS, RanAt: fromAt(i.Task.RanAt), Why: i.Task.Why}
	}
	return v
}

var (
	names    = []string{"api", "web", "db", "cache", "worker", "orders", "gateway", "queue", "migrate", "seed", "kafka", "aws", "search", "mail", "x"}
	kinds    = []string{"app", "service", "module", "task", "", "lambda"}
	states   = []string{"active", "idle", "booting", "crashing", "failed", "error", "reaped", "", "stopping"}
	runs     = []string{"", "go run .", "docker run -p 8080:80 nginx", "podman run --name x -e A=1 redis:7", "docker run -d", "docker run --rm -v a:b postgres:17 -c x", "docker  run\timage", "npm start", "docker", "dockerrun x", "docker run --env=X=1 img", "docker run -p"}
	engines  = []string{"", "postgres", "valkey", " spaced "}
	versions = []string{"", "17.2", "builtin", "builtin 1.0", "builtin "}
	errs     = []string{"", "", "boom", "line one\nline two", "\x1b[31mred\x1b[0m error\r\nwith cr", "exit status 1", "tab\there\x07bell"}
	whys     = []string{"", "you asked", "api needs it", "\x1b[31mred\x1b[0m\u200b"}
	tooks    = []int64{0, 1, 49, 50, 149, 150, 999, 1000, 1049, 1050, 61000, 3600000, 3 * 86400000}
	rams     = []int64{0, 1, 524287, 524288, 84 << 20, 1<<30 - 1, 1 << 30, 1288490188, 5 << 30}
	idles    = []time.Duration{0, time.Second, 30 * time.Second, 5 * time.Minute, time.Hour, 90*time.Second + 500*time.Millisecond}
)

func oneOf(r *rand.Rand, xs ...string) string { return pick(r, xs) }

func instance(r *rand.Rand, name string, all []string) instIn {
	i := instIn{
		Name: name, Kind: pick(r, kinds), State: pick(r, states), Run: pick(r, runs), Engine: pick(r, engines),
		Version: pick(r, versions), StartedAt: at(when(r)), IdleSince: at(when(r)), NextRestart: at(when(r)),
		IdleAfter: int64(pick(r, idles)), AlwaysOn: r.IntN(5) == 0, NameOnly: r.IntN(8) == 0,
		HasConsole: r.IntN(4) == 0, Tainted: r.IntN(10) == 0, Disabled: r.IntN(10) == 0, Paused: r.IntN(12) == 0,
		KeepAwake: r.IntN(8) == 0, RAM: pick(r, rams),
		Endpoint:     oneOf(r, "", "127.0.0.1:5432", "http://localhost:8080", "unix:/tmp/x.sock", "http://", "db.shop.doze"),
		Bind:         oneOf(r, "", "127.0.0.11:5432", "unix:/tmp/s", "0.0.0.0:80", ":8080"),
		Domain:       oneOf(r, "", "", "api.shop.doze", "w\x1b]0;t\x07eb.doze"),
		Console:      oneOf(r, "", "", "", "http://localhost:8233"),
		ConsoleFront: oneOf(r, "", "console", "web"),
		WebURL:       oneOf(r, "", "", "", "http://ui.doze"),
		NameHeld:     oneOf(r, "", "", "", "aws.shop.doze is held by doze-aws (pid 412)"),
		URL:          oneOf(r, "", "", "", "postgres://u@db.shop.doze:5432/x"),
		Resource:     oneOf(r, "", "", "", "http://s3.shop.doze/bucket"),
		DataDir:      oneOf(r, "", "/home/dev/.doze/data/db", "/srv/data", "/home/devx/y", "/home/dev"),
		LastError:    pick(r, errs),
		RestartCount: r.IntN(5), RestartMax: pick(r, []int{0, 0, 3, 5}),
	}
	for range r.IntN(4) {
		i.Needs = append(i.Needs, oneOf(r, pick(r, all), pick(r, all), "ghost"))
	}
	for range r.IntN(4) {
		switch r.IntN(4) {
		case 0, 1:
			i.Holders = append(i.Holders, holderIn{Kind: "dependant", Name: pick(r, all)})
		case 2:
			i.Holders = append(i.Holders, holderIn{Kind: "connection", Count: r.IntN(4)})
		default:
			i.Holders = append(i.Holders, holderIn{Kind: oneOf(r, "pin", "weird")})
		}
	}
	if i.Kind == "task" || r.IntN(10) == 0 {
		if r.IntN(4) > 0 {
			i.Task = &taskIn{Result: oneOf(r, "done", "failed", "failed", ""), Exit: pick(r, []int{0, 1, -1, 137}),
				TookMS: pick(r, tooks), RanAt: at(when(r)), Why: pick(r, whys)}
		}
	}
	return i
}

type snapIn struct {
	Idle      int64    `json:"idle,omitempty"`
	Instances []instIn `json:"instances,omitempty"`
}

func snapCase(o *out, r *rand.Rand) {
	n := 1 + r.IntN(7)
	perm := r.Perm(len(names))[:n]
	var own []string
	for _, k := range perm {
		own = append(own, names[k])
	}
	in := snapIn{Idle: int64(pick(r, idles))}
	for _, name := range own {
		in.Instances = append(in.Instances, instance(r, name, own))
	}
	snap := control.Snapshot{IdleTimeout: time.Duration(in.Idle)}
	for _, i := range in.Instances {
		snap.Instances = append(snap.Instances, i.view())
	}
	var each []map[string]any
	for _, v := range snap.Instances {
		each = append(each, instanceOut(v, snap))
	}
	rows := display.Rows(snap)
	var listed, tree [][2]any
	for _, row := range rows {
		listed = append(listed, [2]any{row.V.Name, row.Group})
	}
	ordered, depth := display.TreeOrder(rows)
	for _, row := range ordered {
		tree = append(tree, [2]any{row.V.Name, depth[row.V.Name]})
	}
	t := display.TallyOf(snap.Instances)
	o.emit("snap", in, map[string]any{
		"each": each, "rows": list(listed), "tree": list(tree),
		"tally": []int64{int64(t.Awake), int64(t.Dozing), int64(t.Waking), int64(t.Asleep), int64(t.Off), int64(t.Paused), int64(t.Trouble), int64(t.Failed), t.RAM, int64(t.Conns)},
	})
}
