package main

import (
	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/display"
)

func look(l display.Look) []string { return []string{l.Word, l.Glyph, string(l.Tone)} }

// instanceOut is everything said about one instance of a snapshot.
func instanceOut(v control.InstanceView, snap control.Snapshot) map[string]any {
	addr, port := display.URL(v)
	var needs [][]any
	for _, n := range display.Needs(v, snap) {
		needs = append(needs, []any{n.Name, n.Known, look(n.Look), n.Glyph, n.Task})
	}
	note, bad := display.Note(v, snap.IdleTimeout, now)
	var held [][]any
	for _, s := range display.HeldSentence(v) {
		held = append(held, []any{s.Text, s.Strong})
	}
	var facts [][]string
	for _, f := range display.ResetFacts(v) {
		facts = append(facts, []string{f.Key, f.Text, string(f.Tone)})
	}
	var tabs [][]string
	for _, t := range display.Tabs(v, nil) {
		tabs = append(tabs, []string{t.ID, t.Title})
	}
	c := display.CleanView(v)
	cleaned := []string{c.Name, c.Run, c.URL, c.Domain, c.LastError, ""}
	var result any
	if v.Task != nil {
		cleaned[5] = c.Task.Why
		text, bad := display.TaskResult(v.Name, v.Task)
		result = []any{text, bad}
	}
	return map[string]any{
		"word": display.Word(v.State), "awake": display.Awake(v.State), "look": look(display.LookOf(v)),
		"awake_now": display.AwakeNow(v), "in_use": display.InUse(v), "troubled": display.Troubled(v),
		"what": display.What(v, snap, now), "because": display.Because(v, snap, now), "runs": display.Runs(v),
		"first_unmet": display.FirstUnmet(v, snap), "holder_named": display.HolderNamed(v), "mem": display.Mem(v),
		"url": []string{addr, port}, "needs": list(needs), "address": display.Address(v), "address_of": display.AddressOf(v),
		"hint": display.Hint(v, snap, now), "switch_on": display.SwitchOn(v), "note": []any{note, bad},
		"state_rank": display.StateRank(v), "tabs": list(tabs), "has_web": display.HasWeb(v), "web_url": display.WebURL(v),
		"dependants": list(display.Dependants(v)), "connections": display.Connections(v),
		"sleep_buttons": display.SleepButtons(v), "restart_buttons": display.RestartButtons(v), "held": list(held),
		"reset_facts": facts, "clean_view": cleaned, "task_result": result, "short_run": display.ShortRun(v.Run),
	}
}

// list is xs, written as [] rather than null when it is empty.
func list[T any](xs []T) []T {
	if xs == nil {
		return []T{}
	}
	return xs
}
