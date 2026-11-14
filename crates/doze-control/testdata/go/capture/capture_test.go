package fixture_test

// Captures for doze-rs's doze-control: every wire type, filled by reflection
// and encoded by Go's own json.Encoder, and each fixture hub as a script file.
//
// To regenerate crates/doze-control/testdata/go/{types,scripts}: copy this file
// and states.go (the states hub: one instance in every state, which doze-rs
// added; register it in devserve's Fixtures as "states" to see it in Go's app)
// into the Go doze repo at internal/control/fixture/, then run
//
//	DOZE_CAPTURE=<doze-rs>/crates/doze-control/testdata/go go test ./internal/control/fixture/ -run TestCapture
//
// testdata/go/live is a session recorded from a real daemon on a throwaway
// stack (one portless app, one manual task) in a temp DOZE_HOME.

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"testing"
	"time"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/control/fakedaemon"
	"github.com/doze-dev/doze/internal/control/fixture"
	"github.com/doze-dev/doze/internal/journal"
)

var epoch = time.Date(2026, 10, 11, 14, 4, 40, 0, time.UTC)

type variant struct {
	name  string
	str   func(field string) string
	num   int64
	unum  uint64
	float float64
	when  time.Time
	dur   time.Duration
}

var variants = []variant{
	{"full", func(f string) string { return f + " <a&b> é\t \x01\"\\" }, 7, 9, 12.5,
		time.Date(2026, 10, 11, 14, 4, 40, 123456789, time.UTC), 1500 * time.Millisecond},
	{"alt", func(f string) string { return "x" }, -3, ^uint64(0), 3,
		time.Date(2026, 1, 2, 3, 4, 5, 120000000, time.FixedZone("IST", 19800)), 42},
	{"small", func(f string) string { return f }, 1, 1, 0.0425,
		time.Date(1999, 12, 31, 23, 59, 59, 0, time.FixedZone("W", -7*3600)), time.Nanosecond},
}

func fill(v reflect.Value, field string, va variant, depth int) {
	switch v.Kind() {
	case reflect.String:
		v.SetString(va.str(field))
	case reflect.Bool:
		v.SetBool(true)
	case reflect.Int, reflect.Int32, reflect.Int64:
		if v.Type() == reflect.TypeOf(time.Duration(0)) {
			v.SetInt(int64(va.dur))
		} else {
			v.SetInt(va.num)
		}
	case reflect.Uint64:
		v.SetUint(va.unum)
	case reflect.Float64:
		v.SetFloat(va.float)
	case reflect.Pointer:
		if depth > 2 {
			return
		}
		p := reflect.New(v.Type().Elem())
		fill(p.Elem(), field, va, depth+1)
		v.Set(p)
	case reflect.Slice:
		if depth > 2 {
			return
		}
		s := reflect.MakeSlice(v.Type(), 2, 2)
		for i := 0; i < 2; i++ {
			fill(s.Index(i), fmt.Sprintf("%s%d", field, i), va, depth+1)
		}
		v.Set(s)
	case reflect.Map:
		m := reflect.MakeMap(v.Type())
		for _, k := range []string{"b", "a", "_for"} {
			m.SetMapIndex(reflect.ValueOf(k), reflect.ValueOf(va.str(field+k)))
		}
		v.Set(m)
	case reflect.Struct:
		if v.Type() == reflect.TypeOf(time.Time{}) {
			v.Set(reflect.ValueOf(va.when))
			return
		}
		for i := 0; i < v.NumField(); i++ {
			if v.Type().Field(i).IsExported() {
				fill(v.Field(i), v.Type().Field(i).Name, va, depth+1)
			}
		}
	}
}

func encodeTo(t *testing.T, path string, values ...any) {
	t.Helper()
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	enc := json.NewEncoder(f)
	for _, v := range values {
		if err := enc.Encode(v); err != nil {
			t.Fatal(err)
		}
	}
}

func TestCaptureTypes(t *testing.T) {
	dir := os.Getenv("DOZE_CAPTURE")
	if dir == "" {
		t.Skip("DOZE_CAPTURE not set")
	}
	types := map[string]any{
		"request": control.Request{}, "hello": control.Hello{}, "log_frame": control.LogFrame{},
		"event_frame": control.EventFrame{}, "proc_view": control.ProcView{}, "resource_view": control.ResourceView{},
		"action_view": control.ActionView{}, "holder_view": control.HolderView{}, "instance_view": control.InstanceView{},
		"response": control.Response{}, "stat_view": control.StatView{}, "frame": control.Frame{},
		"task_view": control.TaskView{}, "console_spec": control.ConsoleSpec{}, "console_tab": control.ConsoleTab{},
		"console_extension": control.ConsoleExtension{}, "console_node": control.ConsoleNode{},
		"console_panel": control.ConsolePanel{}, "console_column": control.ConsoleColumn{},
		"console_row": control.ConsoleRow{}, "console_frame": control.ConsoleFrame{},
		"journal_event": journal.Event{}, "journal_cause": journal.Cause{},
	}
	out := filepath.Join(dir, "types")
	_ = os.MkdirAll(out, 0o755)
	for name, zero := range types {
		encodeTo(t, filepath.Join(out, name+".zero.json"), zero)
		for _, va := range variants {
			v := reflect.New(reflect.TypeOf(zero)).Elem()
			fill(v, "", va, 0)
			encodeTo(t, filepath.Join(out, name+"."+va.name+".json"), v.Interface())
		}
	}
}

// entry is one line of a doze-control fixture script.
type entry struct {
	Status      *control.Response `json:"status,omitempty"`
	Hold        *bool             `json:"hold,omitempty"`
	Frame       *control.Frame    `json:"frame,omitempty"`
	Log         *control.LogFrame `json:"log,omitempty"`
	AdminResult *string           `json:"admin_result,omitempty"`
}

func script(f *fakedaemon.Fake) []any {
	snap, _ := f.Snapshot(nil)
	out := []any{entry{Status: &control.Response{OK: true, Listen: snap.Listen, IdleTimeout: snap.IdleTimeout,
		Stack: snap.Stack, Dir: snap.Dir, Loaded: snap.Loaded, Instances: snap.Instances}}}
	if f.Hold {
		out = append(out, entry{Hold: &f.Hold})
	}
	if f.AdminResult != "" {
		out = append(out, entry{AdminResult: &f.AdminResult})
	}
	for i := range f.Frames {
		out = append(out, entry{Frame: &f.Frames[i]})
	}
	names := make([]string, 0, len(f.Lines))
	for n := range f.Lines {
		names = append(names, n)
	}
	sort.Strings(names)
	for _, n := range names {
		for i := range f.Lines[n] {
			out = append(out, entry{Log: &f.Lines[n][i]})
		}
	}
	return out
}

func TestCaptureScripts(t *testing.T) {
	dir := os.Getenv("DOZE_CAPTURE")
	if dir == "" {
		t.Skip("DOZE_CAPTURE not set")
	}
	hubs := map[string]*fakedaemon.Fake{
		"shop": fixture.Shop(epoch), "crash": fixture.Crash(epoch), "modules": fixture.Modules(epoch),
		"larder": fixture.Larder(epoch), "larder-crash": fixture.LarderCrash(epoch),
		"larder-portclash": fixture.LarderPortClash(epoch), "larder-domains": fixture.LarderDomains(epoch),
		"mess": fixture.Mess(epoch), "states": fixture.States(epoch), "long": fixture.Long(epoch), "empty": {},
	}
	out := filepath.Join(dir, "scripts")
	_ = os.MkdirAll(out, 0o755)
	for name, f := range hubs {
		encodeTo(t, filepath.Join(out, name+".ndjson"), script(f)...)
	}
}
