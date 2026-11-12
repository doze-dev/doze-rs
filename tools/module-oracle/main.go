// Command oracle runs the Go doze modules in-process on a corpus of config blocks and prints their answers as JSON
// lines, for the Rust modules to be compared against (docs/port/modules/PLAN.md step 5).
//
//	oracle describe           every module's Description, one JSON object per engine
//	oracle run corpus.jsonl   for each case {id, engine, version, hcl}: decode, network, connection, attributes,
//	                          objects, plan — against the fixed instance the Rust runner builds too. These modules are
//	                          built on SDK v0.11, before listeners were declarable: each has the default "main" only.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/doze-dev/doze-modules/modules/ferret"
	"github.com/doze-dev/doze-modules/modules/kvrocks"
	"github.com/doze-dev/doze-modules/modules/mariadb"
	"github.com/doze-dev/doze-modules/modules/postgres"
	"github.com/doze-dev/doze-modules/modules/temporal"
	"github.com/doze-dev/doze-modules/modules/valkey"
	"github.com/doze-dev/doze-sdk/engine"
	"github.com/hashicorp/hcl/v2"
	"github.com/hashicorp/hcl/v2/hclparse"
	ctyjson "github.com/zclconf/go-cty/cty/json"
)

var drivers = map[string]engine.Driver{
	"valkey":   valkey.Driver{},
	"kvrocks":  kvrocks.Driver{},
	"postgres": postgres.Driver{},
	"ferret":   ferret.Driver{},
	"mariadb":  mariadb.Driver{},
	"temporal": temporal.Driver{},
}

type corpusCase struct {
	ID      string `json:"id"`
	Engine  string `json:"engine"`
	Version string `json:"version"`
	HCL     string `json:"hcl"`
}

type process struct {
	Name     string            `json:"name"`
	Program  string            `json:"program"`
	Args     []string          `json:"args"`
	EnvAdded map[string]string `json:"env_added"`
	Dir      string            `json:"dir"`
	After    []string          `json:"after"`
	Ready    string            `json:"ready"`
	Hooks    []string          `json:"hooks"`
	Tree     bool              `json:"tree"`
}

type answer struct {
	ID          string                     `json:"id"`
	Engine      string                     `json:"engine"`
	DecodeError string                     `json:"decode_error,omitempty"`
	Backend     string                     `json:"backend,omitempty"`
	EnvVar      string                     `json:"env_var,omitempty"`
	URL         string                     `json:"url,omitempty"`
	Attributes  map[string]json.RawMessage `json:"attributes,omitempty"`
	Objects     []engine.Object            `json:"objects,omitempty"`
	Plan        []process                  `json:"plan,omitempty"`
	PlanError   string                     `json:"plan_error,omitempty"`
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: oracle describe | oracle run corpus.jsonl")
		os.Exit(2)
	}
	out := json.NewEncoder(os.Stdout)
	switch os.Args[1] {
	case "describe":
		names := make([]string, 0, len(drivers))
		for n := range drivers {
			names = append(names, n)
		}
		sort.Strings(names)
		for _, n := range names {
			if d, ok := drivers[n].(engine.Describer); ok {
				must(out.Encode(map[string]any{"engine": n, "description": d.Describe()}))
			}
		}
	case "run":
		must(os.RemoveAll(root))
		f, err := os.Open(os.Args[2])
		must(err)
		sc := bufio.NewScanner(f)
		sc.Buffer(make([]byte, 1<<20), 1<<24)
		for sc.Scan() {
			var c corpusCase
			must(json.Unmarshal(sc.Bytes(), &c))
			must(out.Encode(run(c)))
		}
		must(sc.Err())
	}
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, "oracle:", err)
		os.Exit(1)
	}
}

// decode does what the Go plugin server's DecodeConfig does: parse, find the block, strip the core's arguments,
// decode with no variables.
func decode(d engine.Driver, c corpusCase) (string, any, error) {
	file, diags := hclparse.NewParser().ParseHCL([]byte(c.HCL), "case.hcl")
	if diags.HasErrors() {
		return "", nil, diags
	}
	content, _, diags := file.Body.PartialContent(&hcl.BodySchema{Blocks: []hcl.BlockHeaderSchema{{Type: c.Engine, LabelNames: []string{"name"}}}})
	if diags.HasErrors() || len(content.Blocks) != 1 {
		return "", nil, fmt.Errorf("expected one %s block: %v", c.Engine, diags)
	}
	block := content.Blocks[0]
	core := &hcl.BodySchema{}
	for _, a := range engine.CoreArguments {
		core.Attributes = append(core.Attributes, hcl.AttributeSchema{Name: a})
	}
	_, remain, diags := block.Body.PartialContent(core)
	if diags.HasErrors() {
		return "", nil, diags
	}
	dec, ok := d.(engine.ConfigDecoder)
	if !ok {
		return block.Labels[0], nil, nil
	}
	spec, err := dec.DecodeConfig(remain, &hcl.EvalContext{}, "/base", engine.VersionSpec(c.Version))
	return block.Labels[0], spec, err
}

func run(c corpusCase) answer {
	a := answer{ID: c.ID, Engine: c.Engine}
	d, ok := drivers[c.Engine]
	if !ok {
		a.DecodeError = "no such engine"
		return a
	}
	name, spec, err := decode(d, c)
	if err != nil {
		a.DecodeError = oneLine(err.Error())
		return a
	}
	inst, ep := fixedInstance(d, c, name, spec)
	a.Backend = ep.Backend
	a.EnvVar, a.URL = d.ConnString(inst, ep)
	if at, ok := d.(engine.Attributer); ok {
		a.Attributes = map[string]json.RawMessage{}
		for k, v := range at.Attributes(inst, ep) {
			b, err := ctyjson.Marshal(v, v.Type())
			if err == nil {
				a.Attributes[k] = b
			}
		}
	}
	if inv, ok := d.(engine.Inventory); ok {
		a.Objects = inv.Objects(inst)
	}
	plan(d, inst, &a)
	return a
}

// fixedInstance is the instance the Rust runner builds too: the same paths, names and addresses.
// root holds the fixed instance's directories (plans create them); the Rust runner uses the same paths.
const root = "/tmp/dzo"

func fixedInstance(d engine.Driver, c corpusCase, name string, spec any) (engine.Instance, engine.Endpoint) {
	socketDir := root + "/s/" + name
	ep := engine.Endpoint{TCPAddr: "127.0.0.1:7777", Backend: d.BackendSocket(socketDir, 5432)}
	inst := engine.Instance{
		Name: name, Type: c.Engine, Version: engine.VersionSpec(c.Version),
		DataDir: root + "/d/" + name, SocketDir: socketDir, Port: 5432, Endpoint: ep, Spec: spec,
		Toolchain: engine.Toolchain{Engine: c.Engine, Full: "9.9.9", BinDir: "/tc/bin"},
	}
	return inst, ep
}

func plan(d engine.Driver, inst engine.Instance, a *answer) {
	sp, ok := d.(engine.Spawner)
	if !ok {
		a.PlanError = "not a Spawner"
		return
	}
	p, err := sp.Plan(context.Background(), inst, inst.Toolchain)
	if err != nil {
		a.PlanError = oneLine(err.Error())
		return
	}
	inherited := map[string]bool{}
	for _, kv := range os.Environ() {
		inherited[kv] = true
	}
	for _, s := range p.Specs {
		pr := process{Name: s.Name, Program: s.Bin, Args: s.Args, Dir: s.Dir, After: s.After, Hooks: s.Hooks, Tree: s.Tree, EnvAdded: map[string]string{}}
		for _, kv := range s.Env {
			if !inherited[kv] {
				k, v, _ := strings.Cut(kv, "=")
				pr.EnvAdded[k] = v
			}
		}
		if s.Ready != nil {
			pr.Ready = s.Ready.Kind + " " + s.Ready.Target
		}
		a.Plan = append(a.Plan, pr)
	}
}

func oneLine(s string) string { return strings.Join(strings.Fields(s), " ") }
