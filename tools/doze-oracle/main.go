// The Go doze's own answers for doze-projects and doze-journal (docs/port/app/PLAN.md, step 0.3): it writes project
// records and journals with the Go code that writes them in production, and prints what the Go code reads back, so
// the Rust readers can be held to both.
//
// The packages it uses are internal to github.com/doze-dev/doze, so it cannot be its own module. It is built inside
// a doze checkout with an overlay that places this file there for the build only (the checkout is not changed):
//
//	tools/doze-oracle/run.sh <doze checkout> <command> <args...>
//
// Commands:
//
//	projects <home>    records three projects (one with a field a newer doze wrote, one broken) under <home>,
//	                   then prints projects.List(<home>) as JSON lines
//	journal <path>     appends events with rotation at 600 bytes, tears the last line, then prints journal.Read
//	                   as JSON lines
//	lines <file>       reads each line of <file> as a journal of its own; prints the event Go reads as JSON, or skip
//	slug <config>...   prints config.ProjectDirIn("/h", config) for each config path
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/doze-dev/doze/internal/config"
	"github.com/doze-dev/doze/internal/journal"
	"github.com/doze-dev/doze/internal/projects"
)

// at is the fixed clock every record uses, so fixtures are the same on every run.
var at = time.Date(2026, 11, 13, 9, 30, 0, 123456789, time.UTC)

func main() {
	var err error
	switch os.Args[1] {
	case "projects":
		err = writeProjects(os.Args[2])
	case "journal":
		err = writeJournal(os.Args[2])
	case "lines":
		err = readLines(os.Args[2])
	case "slug":
		for _, c := range os.Args[2:] {
			fmt.Println(config.ProjectDirIn("/h", c))
		}
	default:
		err = fmt.Errorf("unknown command %q", os.Args[1])
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func writeProjects(home string) error {
	dir := func(s string) string { return filepath.Join(home, "projects", s) }
	if err := projects.Update(dir("shop-1a2b3c4d"), func(p *projects.Project) {
		p.Config, p.Name, p.LastUp, p.LastSet = "/code/shop/doze.hcl", "shop", at, []string{"api", "worker"}
	}); err != nil {
		return err
	}
	// A local-time stamp with an offset, and a down after the up: listed by LastDown.
	east := time.FixedZone("", 5*3600+30*60)
	if err := projects.Update(dir("blog-00ff00ff"), func(p *projects.Project) {
		p.Config, p.LastUp, p.LastDown = "/code/blog/doze.hcl", at.Add(-2*time.Hour).In(east), at.Add(-time.Minute).In(east)
	}); err != nil {
		return err
	}
	// A field a newer doze wrote, kept through an update by this one.
	newer := dir("harbour-12345678")
	if err := os.MkdirAll(newer, 0o755); err != nil {
		return err
	}
	raw := `{"schema":2,"config":"/code/harbour/doze.hcl","tabs":["home",{"pinned":true}],"last_up":"2026-11-12T08:00:00Z"}`
	if err := os.WriteFile(filepath.Join(newer, projects.FileName), []byte(raw), 0o644); err != nil {
		return err
	}
	if err := projects.Update(newer, func(p *projects.Project) { p.Name = "harbour" }); err != nil {
		return err
	}
	// A broken record, which List skips.
	bad := dir("bad-00000000")
	if err := os.MkdirAll(bad, 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(bad, projects.FileName), []byte(`{"config":`), 0o644); err != nil {
		return err
	}
	list, err := projects.List(home)
	if err != nil {
		return err
	}
	for _, p := range list {
		if err := printJSON(struct {
			Dir string `json:"dir"`
			projects.Project
		}{strings.TrimPrefix(p.Dir(), home), p}); err != nil {
			return err
		}
	}
	return nil
}

func writeJournal(path string) error {
	j, err := journal.Open(path, 600)
	if err != nil {
		return err
	}
	events := []journal.Event{
		{Kind: journal.Wake, Instance: "db", Cause: journal.Cause{Kind: journal.ByDependency, By: "api"}},
		{Kind: journal.Ready, Instance: "db", Took: 38 * time.Millisecond},
		{Kind: journal.Wake, Instance: "api", Cause: journal.Cause{Kind: journal.ByConnection}},
		{Kind: journal.Step, Instance: "api", Detail: "setup: migrate \"quoted\" éè \U0001F600"},
		{Kind: journal.Crash, Instance: "worker", Detail: "exit status 2"},
		{Kind: journal.Restart, Instance: "worker", Cause: journal.Cause{Kind: journal.ByRestart}},
		{Kind: journal.GiveUp, Instance: "worker"},
		{Kind: journal.Pin, Instance: "db", Detail: "on"},
		{Kind: journal.Sleep, Instance: "api", Cause: journal.Cause{Kind: journal.ByIdle}},
		{Kind: journal.Failed, Instance: "cache", Detail: "port 6379 in use"},
		{Kind: journal.Wake, Instance: "web", Cause: journal.Cause{Kind: journal.ByWatch, By: "src/app.ts"}},
		{Kind: journal.Sleep, Instance: "db", Cause: journal.Cause{Kind: journal.ByShutdown}},
	}
	for i, e := range events {
		e.Time = at.Add(time.Duration(i) * time.Second)
		j.Append(e)
	}
	if err := j.Close(); err != nil {
		return err
	}
	// A write cut short by a crash.
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	if _, err := f.WriteString(`{"seq":13,"time":"2026-11-13T09:30:12Z","kind":"wa`); err != nil {
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	all, err := journal.Read(path)
	if err != nil {
		return err
	}
	for _, e := range all {
		if err := printJSON(e); err != nil {
			return err
		}
	}
	return nil
}

func printJSON(v any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	_, err = fmt.Println(string(b))
	return err
}

// readLines reads each line of file as a one-line journal, so a test can ask Go what it makes of any line.
func readLines(file string) error {
	raw, err := os.ReadFile(file)
	if err != nil {
		return err
	}
	dir, err := os.MkdirTemp("", "doze-oracle")
	if err != nil {
		return err
	}
	defer func() { _ = os.RemoveAll(dir) }()
	path := filepath.Join(dir, "j.jsonl")
	for _, line := range strings.Split(strings.TrimSuffix(string(raw), "\n"), "\n") {
		if err := os.WriteFile(path, []byte(line), 0o644); err != nil {
			return err
		}
		events, err := journal.Read(path)
		if err != nil {
			return err
		}
		if len(events) != 1 {
			fmt.Println("skip")
			continue
		}
		if err := printJSON(events[0]); err != nil {
			return err
		}
	}
	return nil
}
