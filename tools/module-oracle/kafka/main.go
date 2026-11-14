// Command kafka-oracle runs the Go kafka module (doze-kafka/module, SDK v0.12) in-process on a corpus of config
// blocks and prints its answers as JSON lines, in the shape of ../main.go's, for the Rust module face of doze-kafka
// to be compared against (docs/port/phase11.md). It is its own Go module because the kafka module needs SDK v0.12
// (granted listeners) and the broker it embeds, which the other six modules' oracle (SDK v0.11) does not.
//
//	kafka-oracle describe           the module's Description, as one JSON object
//	kafka-oracle run corpus.jsonl   for each case {id, engine, version, hcl}: decode, network, connection, objects and
//	                                plan against a fixed instance, granted as a v0.12 doze grants it
//
// The fixed instance: data /tmp/dzo/d/<name>, sockets /tmp/dzo/s/<name>, the main listener at 127.0.0.1:7777 and the
// console's and the registry's at their own ports, each with a backend socket, and the toolchain Resolve gives.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	kafka "github.com/doze-dev/doze-kafka/module"
	"github.com/doze-dev/doze-sdk/engine"
	"github.com/hashicorp/hcl/v2"
	"github.com/hashicorp/hcl/v2/hclparse"
)

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

type listener struct {
	Name   string   `json:"name"`
	Port   int      `json:"port"`
	HTTP   bool     `json:"http"`
	Routes []string `json:"routes"`
}

type answer struct {
	ID          string          `json:"id"`
	Engine      string          `json:"engine"`
	DecodeError string          `json:"decode_error,omitempty"`
	Backend     string          `json:"backend,omitempty"`
	Network     []listener      `json:"network,omitempty"`
	EnvVar      string          `json:"env_var,omitempty"`
	URL         string          `json:"url,omitempty"`
	Objects     []engine.Object `json:"objects,omitempty"`
	Plan        []process       `json:"plan,omitempty"`
	PlanError   string          `json:"plan_error,omitempty"`
}

// root holds the fixed instance's directories (Plan creates the socket directory); the Rust side uses the same paths.
const root = "/tmp/dzo"

var driver = kafka.New()

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: kafka-oracle describe | kafka-oracle run corpus.jsonl")
		os.Exit(2)
	}
	out := json.NewEncoder(os.Stdout)
	switch os.Args[1] {
	case "describe":
		must(out.Encode(map[string]any{"engine": "kafka", "description": driver.Describe()}))
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
		fmt.Fprintln(os.Stderr, "kafka-oracle:", err)
		os.Exit(1)
	}
}

// decode does what the Go plugin server's DecodeConfig does: parse, find the block, strip the core's arguments,
// decode with no variables.
func decode(c corpusCase) (string, engine.EngineConfig, error) {
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
	spec, err := driver.DecodeConfig(remain, &hcl.EvalContext{}, "/base", engine.VersionSpec(c.Version))
	return block.Labels[0], spec, err
}

func run(c corpusCase) answer {
	a := answer{ID: c.ID, Engine: c.Engine}
	name, spec, err := decode(c)
	if err != nil {
		a.DecodeError = oneLine(err.Error())
		return a
	}
	inst, ep := fixedInstance(c, name, spec)
	a.Backend = ep.Backend
	for _, l := range driver.Network(inst).Listeners {
		a.Network = append(a.Network, listener{Name: l.Name, Port: l.Port, HTTP: l.Protocol == engine.ProtocolHTTP, Routes: l.Routes})
	}
	a.EnvVar, a.URL = driver.ConnString(inst, ep)
	tc, err := driver.Resolve(context.Background(), inst.Version, engine.Platform{}, nil, nil)
	if err != nil {
		a.PlanError = oneLine(err.Error())
		return a
	}
	plan(inst, tc, &a)
	return a
}

// fixedInstance is the instance the Rust side builds too: the same paths and names, and the listeners granted as a
// v0.12 doze grants them (each with a backend socket the host forwards its port to).
func fixedInstance(c corpusCase, name string, spec engine.EngineConfig) (engine.Instance, engine.Endpoint) {
	socketDir := root + "/s/" + name
	host := name + ".shop.doze"
	ep := engine.Endpoint{TCPAddr: "127.0.0.1:7777", Backend: driver.BackendSocket(socketDir, 9092)}
	sock := func(l string) string { return socketDir + "/" + l + ".sock" }
	grant := engine.Grant{Host: host, Listeners: map[string]engine.GrantedListener{
		engine.MainListener: {Addr: "127.0.0.1:7777", Backend: sock("main")},
		"console":           {Addr: "127.0.0.1:" + strconv.Itoa(9093), URL: "http://" + host, Hosts: []string{host}, Backend: sock("console")},
		"registry":          {Addr: "127.0.0.1:" + strconv.Itoa(8081), URL: "http://registry." + host, Hosts: []string{"registry." + host}, Backend: sock("registry")},
	}}
	inst := engine.Instance{
		Name: name, Type: c.Engine, Version: engine.VersionSpec(c.Version),
		DataDir: root + "/d/" + name, SocketDir: socketDir, Port: 9092, Endpoint: ep, Spec: spec, Grant: grant,
	}
	return inst, ep
}

func plan(inst engine.Instance, tc engine.Toolchain, a *answer) {
	inst.Toolchain = tc
	p, err := driver.Plan(context.Background(), inst, tc)
	if err != nil {
		a.PlanError = oneLine(err.Error())
		return
	}
	inherited := map[string]bool{}
	for _, kv := range os.Environ() {
		inherited[kv] = true
	}
	for _, s := range p.Specs {
		pr := process{Name: s.Name, Program: "<self>", Args: s.Args, Dir: s.Dir, After: s.After, Hooks: s.Hooks, Tree: s.Tree, EnvAdded: map[string]string{}}
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
