package main

import (
	"encoding/json"
	"math/rand/v2"
	"sort"
	"time"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/display"
)

var (
	roles      = []string{"count", "bytes", "rate", "lag", "state", "time", "", "weird"}
	panelKinds = []string{"table", "counters", "stream", "odd"}
	nodeKinds  = []string{"row", "col", "table", "stat", "detail", "stream", "actions", "note", "chart", "", "odd"}
	keys       = []string{"name", "from", "msgs", "bytes", "lag", "state", "rate", "size"}
	// cell values: figures with commas, signs, Go-only number forms, states in any case, and text. No value Go
	// would turn into an int64 from out of range (that conversion is the platform's choice).
	cellValues = []string{"", "0", "12", "-5", "1,043", "1,234,567", "3.7", "-0.4", "1_000", "0x1p4", "1e400", "abc",
		"OK", "Waiting", "dead", "Stable", "weird", "12345678901", "+7", "1e3", "Infinity", "NaN", ".5"}
)

func randomSpec(r *rand.Rand) *control.ConsoleSpec {
	spec := &control.ConsoleSpec{}
	for i := range 1 + r.IntN(4) {
		p := control.ConsolePanel{ID: oneOf(r, "topics", "groups", "brokers", "log") + string(rune('a'+i)), Title: pick(r, words), Kind: pick(r, panelKinds), Select: oneOf(r, "", "topicsa")}
		for range r.IntN(5) {
			key := pick(r, keys)
			title := key
			if r.IntN(3) == 0 {
				title = pick(r, words)
			}
			p.Columns = append(p.Columns, control.ConsoleColumn{Key: key, Title: title, Role: pick(r, roles)})
		}
		spec.Panels = append(spec.Panels, p)
	}
	if r.IntN(2) == 0 {
		for _, id := range []string{"main", "more"}[:1+r.IntN(2)] {
			spec.Tabs = append(spec.Tabs, control.ConsoleTab{ID: id, Title: pick(r, words), Body: randomNode(r, spec, 0)})
		}
	}
	return spec
}

func randomNode(r *rand.Rand, spec *control.ConsoleSpec, depth int) control.ConsoleNode {
	n := control.ConsoleNode{Kind: pick(r, nodeKinds), ID: oneOf(r, "", "", "t1", "d1"), Title: pick(r, words), Weight: r.IntN(3)}
	n.Panel = pick(r, spec.Panels).ID
	if r.IntN(3) == 0 {
		n.Panel = ""
	}
	n.From = oneOf(r, "", "", "t1", "table:"+pick(r, spec.Panels).ID, pick(r, spec.Panels).ID, "nowhere")
	if depth < 3 && (n.Kind == "row" || n.Kind == "col" || r.IntN(5) == 0) {
		for range r.IntN(4) {
			n.Children = append(n.Children, randomNode(r, spec, depth+1))
		}
	}
	return n
}

type nodeOut struct {
	Kind     string    `json:"kind"`
	ID       string    `json:"id"`
	Title    string    `json:"title"`
	Weight   int       `json:"weight"`
	Panel    string    `json:"panel"`
	From     string    `json:"from"`
	Children []nodeOut `json:"children"`
}

func tree(n control.ConsoleNode) nodeOut {
	out := nodeOut{Kind: n.Kind, ID: n.ID, Title: n.Title, Weight: n.Weight, Panel: n.Panel, From: n.From, Children: []nodeOut{}}
	for _, c := range n.Children {
		out.Children = append(out.Children, tree(c))
	}
	return out
}

func randomRow(r *rand.Rand, spec *control.ConsoleSpec) control.ConsoleRow {
	row := control.ConsoleRow{Cells: map[string]string{}, Tone: oneOf(r, "", "", "dim", "bad")}
	for _, k := range keys {
		if r.IntN(4) > 0 {
			row.Cells[k] = pick(r, cellValues)
		}
	}
	if r.IntN(4) == 0 {
		row.Cells[control.CellRuntime] = "1"
	}
	return row
}

func sorted(cells map[string]string) [][2]string {
	out := [][2]string{}
	for k, v := range cells {
		out = append(out, [2]string{k, v})
	}
	sort.Slice(out, func(i, j int) bool { return out[i][0] < out[j][0] })
	return out
}

func consoleCase(o *out, r *rand.Rand) {
	spec := randomSpec(r)
	if r.IntN(10) == 0 {
		spec = nil
	}
	tables := map[string][]control.ConsoleRow{}
	sel := map[string]int{}
	if spec != nil {
		for _, p := range spec.Panels {
			if r.IntN(5) > 0 {
				for range r.IntN(4) {
					tables[p.ID] = append(tables[p.ID], randomRow(r, spec))
				}
			}
		}
		sel["t1"], sel["table:"+spec.Panels[0].ID] = r.IntN(4), r.IntN(4)
	}
	// the input as it stands now: the rate steps below change cells, as a module's next frame would
	in, err := json.Marshal(map[string]any{"spec": spec, "tables": tables, "sel": sel})
	if err != nil {
		panic(err)
	}
	got := map[string]any{}
	bodies := map[string]any{}
	var walked []any
	for _, tab := range []string{"console:_", "console:main", "console:more", "logs"} {
		body, ok := display.ConsoleBody(spec, tab)
		if !ok {
			bodies[tab] = nil
			continue
		}
		bodies[tab] = tree(body)
		display.Walk(body, func(n control.ConsoleNode) {
			walked = append(walked, []any{display.BlockID(n), display.Weight(n), display.Flexible(n), display.HasStream(n),
				detail(display.DetailRows(spec, tab, tables, sel, n))})
		})
	}
	got["bodies"], got["walked"] = bodies, list(walked)
	var tabs [][]string
	for _, t := range display.ConsoleTabs(spec) {
		tabs = append(tabs, []string{t.ID, t.Title})
	}
	got["console_tabs"] = list(tabs)
	var perInstance [][][]string
	for _, front := range []string{"", "console", "web"} {
		for _, kind := range []string{"module", "task"} {
			var ts [][]string
			for _, t := range display.Tabs(control.InstanceView{Kind: kind, HasConsole: true, ConsoleFront: front}, spec) {
				ts = append(ts, []string{t.ID, t.Title})
			}
			perInstance = append(perInstance, ts)
		}
	}
	got["tabs"] = perInstance
	got["cells"] = cellsOut(spec, tables)
	steps, said := ratesOut(r, spec, tables)
	got["rates"] = said
	o.emit("console", map[string]any{"given": json.RawMessage(in), "steps": steps}, got)
}

func detail(rows [][2]string) [][2]string {
	if rows == nil {
		return [][2]string{}
	}
	return rows
}

func cellsOut(spec *control.ConsoleSpec, tables map[string][]control.ConsoleRow) [][]any {
	out := [][]any{}
	if spec == nil {
		return out
	}
	for _, p := range spec.Panels {
		for _, row := range tables[p.ID] {
			for _, col := range p.Columns {
				text, tone := display.Cell(col, row)
				out = append(out, []any{text, string(tone)})
			}
		}
	}
	return out
}

// rateSet is one cell a rate step writes before Rates reads the rows: the panel, the row, the column key, the value.
type rateSet struct {
	Panel string `json:"panel"`
	Row   int    `json:"row"`
	Key   string `json:"key"`
	Value string `json:"value"`
}

// The times the rate steps read at, after now: a second apart, then no time apart, then on.
var rateSteps = []time.Duration{0, time.Second, 2500 * time.Millisecond, 2500 * time.Millisecond, 4 * time.Second}

// ratesOut runs Rates over each panel once per step, writing new totals before each step but the first (they grow,
// fall, and stop being figures), and says the rows each time. The writes are returned for the reader to replay.
func ratesOut(r *rand.Rand, spec *control.ConsoleSpec, tables map[string][]control.ConsoleRow) ([][]rateSet, [][][2]string) {
	steps, said := [][]rateSet{}, [][][2]string{}
	if spec == nil {
		return steps, said
	}
	prev := map[string]display.RateSample{}
	for _, p := range spec.Panels {
		rows := tables[p.ID]
		for i, step := range rateSteps {
			sets := []rateSet{}
			for ri, row := range rows {
				for _, col := range p.Columns {
					if col.Role == "rate" && i > 0 {
						v := pick(r, []string{"100", "1,100", "250", "50", "abc", "2.5"})
						row.Cells[col.Key] = v
						sets = append(sets, rateSet{p.ID, ri, col.Key, v})
					}
				}
			}
			steps = append(steps, sets)
			rowsSaid := [][2]string{}
			for _, row := range display.Rates(spec, p.ID, rows, prev, now.Add(step)) {
				rowsSaid = append(rowsSaid, sorted(row.Cells)...)
				rowsSaid = append(rowsSaid, [2]string{"|", row.Tone})
			}
			said = append(said, rowsSaid)
		}
	}
	return steps, said
}
