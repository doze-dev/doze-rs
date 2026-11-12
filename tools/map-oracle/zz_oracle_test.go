package view

import (
	"encoding/json"
	"math/rand"
	"os"
	"strconv"
	"testing"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/control/fixture"
	"github.com/doze-dev/doze/internal/display"
)

// oracleIn is what the Rust doze-map crate takes for one instance.
type oracleIn struct {
	Name   string   `json:"name"`
	Kind   string   `json:"kind"`
	Needs  []string `json:"needs"`
	Run    string   `json:"run,omitempty"`
	Engine string   `json:"engine,omitempty"`
	// Busy: a task running or failed (a card); Ran: a task that has run.
	Busy bool `json:"busy,omitempty"`
	Ran  bool `json:"ran,omitempty"`
}

type oracleCase struct {
	Name string     `json:"name"`
	In   []oracleIn `json:"in"`
	Map  MapView    `json:"map"`
}

func toOracle(rows []display.Row) []oracleIn {
	var out []oracleIn
	for _, r := range rows {
		v := r.V
		out = append(out, oracleIn{Name: v.Name, Kind: v.Kind, Needs: v.Needs, Run: v.Run, Engine: v.Engine, Busy: v.Kind == "task" && taskBusy(v), Ran: v.Task != nil})
	}
	return out
}

func randomStack(rng *rand.Rand) []control.InstanceView {
	words := []string{"api", "web", "orders", "catalog", "gateway", "identity", "recommendations-engine", "svc", "billing", "notifications-worker", "ledger", "search-indexer-v2", "a", "fulfilment", "inventory"}
	runs := []string{"go run .", "bun src/index.ts", "python3 app.py", "./bin/svc", "pnpm dev", "npm run dev -- x.ts", ""}
	engines := []string{"postgres", "kafka", "redis", "nats", "mysql", "sqs"}
	var in []control.InstanceView
	nApps, nMods, nSvc, nTasks := rng.Intn(11), rng.Intn(7), rng.Intn(4), rng.Intn(5)
	name := func(prefix string, i int) string {
		return words[rng.Intn(len(words))] + "-" + prefix + strconv.Itoa(i)
	}
	var apps, shared, tasks []string
	for i := 0; i < nApps; i++ {
		apps = append(apps, name("a", i))
	}
	for i := 0; i < nMods; i++ {
		shared = append(shared, name("m", i))
	}
	for i := 0; i < nSvc; i++ {
		shared = append(shared, name("s", i))
	}
	for i := 0; i < nTasks; i++ {
		tasks = append(tasks, name("t", i))
	}
	callP := rng.Float64() * 0.35
	for _, a := range apps {
		var needs []string
		for _, b := range apps {
			if rng.Float64() < callP {
				needs = append(needs, b)
			}
		}
		for _, s := range shared {
			if rng.Float64() < 0.4 {
				needs = append(needs, s)
			}
		}
		for _, t := range tasks {
			if rng.Float64() < 0.3 {
				needs = append(needs, t)
			}
		}
		rng.Shuffle(len(needs), func(i, j int) { needs[i], needs[j] = needs[j], needs[i] })
		in = append(in, control.InstanceView{Name: a, Kind: "app", State: "active", Needs: needs, Run: runs[rng.Intn(len(runs))]})
	}
	for i, s := range shared {
		kind := "module"
		if i >= nMods {
			kind = "service"
		}
		in = append(in, control.InstanceView{Name: s, Kind: kind, Engine: engines[rng.Intn(len(engines))]})
	}
	for _, t := range tasks {
		v := control.InstanceView{Name: t, Kind: "task", State: "reaped"}
		switch rng.Intn(4) {
		case 0:
		case 1:
			v.State = "booting"
		case 2:
			v.Task = &control.TaskView{Result: "failed"}
		default:
			v.Task = &control.TaskView{Result: "done"}
		}
		in = append(in, v)
	}
	return in
}

func TestZZOracleDump(t *testing.T) {
	out := os.Getenv("ORACLE_OUT")
	if out == "" {
		t.Skip("ORACLE_OUT not set")
	}
	var cases []oracleCase
	add := func(name string, rows []display.Row) {
		cases = append(cases, oracleCase{Name: name, In: toOracle(rows), Map: mapOf(rows, epoch)})
	}
	add("larder", larderRows())
	add("shop", display.Rows(control.Snapshot{Instances: fixture.Shop(epoch).Snap.Instances}))
	rng := rand.New(rand.NewSource(7))
	for i := 0; i < 400; i++ {
		add("random-"+strconv.Itoa(i), display.Rows(control.Snapshot{Instances: randomStack(rng)}))
	}
	b, err := json.MarshalIndent(cases, "", " ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(out, b, 0o644); err != nil {
		t.Fatal(err)
	}
}

type slimNode struct {
	Name  string   `json:"name"`
	X     int      `json:"x"`
	Y     int      `json:"y"`
	W     int      `json:"w"`
	H     int      `json:"h"`
	Lane  bool     `json:"lane,omitempty"`
	Task  bool     `json:"task,omitempty"`
	Lang  string   `json:"lang,omitempty"`
	Chips []string `json:"chips"`
	More  int      `json:"more,omitempty"`
	Quiet bool     `json:"quiet,omitempty"`
	Fold  bool     `json:"fold,omitempty"`
}
type slimShared struct {
	Name string `json:"name"`
	X    int    `json:"x"`
	Y    int    `json:"y"`
	W    int    `json:"w"`
	H    int    `json:"h"`
}
type slimMap struct {
	W      int          `json:"w"`
	H      int          `json:"h"`
	Caps   []MapCap     `json:"caps"`
	Band   int          `json:"band"`
	Nodes  []slimNode   `json:"nodes"`
	Shared []slimShared `json:"shared"`
	Edges  []MapEdge    `json:"edges"`
	Lines  []MapEdge    `json:"lines"`
	Detail MapAt        `json:"detail"`
}
type slimCase struct {
	Name string     `json:"name"`
	In   []oracleIn `json:"in"`
	Map  slimMap    `json:"map"`
}

func slim(m MapView) slimMap {
	s := slimMap{W: m.W, H: m.H, Caps: m.Caps, Band: m.Band, Edges: m.Edges, Lines: m.Lines, Detail: m.Detail, Nodes: []slimNode{}, Shared: []slimShared{}}
	for _, n := range m.Nodes {
		s.Nodes = append(s.Nodes, slimNode{n.Name, n.X, n.Y, n.W, n.H, n.Lane, n.Task, n.Lang, n.Chips, n.More, n.Quiet != "", n.Fold != nil})
	}
	for _, x := range m.Shared {
		s.Shared = append(s.Shared, slimShared{x.Name, x.X, x.Y, x.W, x.H})
	}
	return s
}

func TestZZOracleSlim(t *testing.T) {
	out := os.Getenv("ORACLE_SLIM")
	if out == "" {
		t.Skip("ORACLE_SLIM not set")
	}
	var cases []slimCase
	add := func(name string, rows []display.Row) {
		cases = append(cases, slimCase{Name: name, In: toOracle(rows), Map: slim(mapOf(rows, epoch))})
	}
	add("larder", larderRows())
	add("shop", display.Rows(control.Snapshot{Instances: fixture.Shop(epoch).Snap.Instances}))
	rng := rand.New(rand.NewSource(7))
	for i := 0; i < 200; i++ {
		add("random-"+strconv.Itoa(i), display.Rows(control.Snapshot{Instances: randomStack(rng)}))
	}
	b, err := json.Marshal(cases)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(out, b, 0o644); err != nil {
		t.Fatal(err)
	}
}
