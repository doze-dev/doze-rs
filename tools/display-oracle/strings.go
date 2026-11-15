package main

import (
	"encoding/hex"
	"errors"
	"math/rand/v2"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/doze-dev/doze/internal/control"
	"github.com/doze-dev/doze/internal/display"
)

// Pieces of what programs print: escape sequences of every family, control bytes, marks that move text, bytes that
// are not UTF-8, and plain text.
var pieces = []string{
	"abc", " ", "hello world", "é", "日本", "😀", "\x1b[31m", "\x1b[0m", "\x1b[38;5;196m", "\x1b]0;title\x07",
	"\x1b]8;;http://x\x1b\\", "\x1bP1$r\x1b\\", "\x1b_apc\x1b\\", "\x1b^pm\x9c", "\x1bXsos\x18", "\x1b(B", "\x1b7",
	"\x1b[?25l", "\x1b[", "\x1b", "\x1b]", "\x9b31m", "\x90q\x9c", "\x9d2;x\x07", "\r", "\n", "\r\n", "\t", "\x00",
	"\x07", "\x7f", "\xc2\x85", "\u200b", "\ufeff", "\u202e", "\u2066", "\u2069", "\xff", "\xe2\x82", "\xc0", "\x80",
	"\xf4\x90\x80\x80", "\xed\xa0\x80", "\xc2", "\xe2\x82\xac", "\x1b\xc3\xa9", "\x1b[3\xe6\x97\xa5m", "\x1b]\xf0\x9f\x98\x80\x07",
}

var levelPieces = []string{
	"level=", "lvl=", "severity=", "loglevel=", `"level":`, `"level": "`, `"severity":"`, `"lvl":`, "warn", "WARNING",
	"error", "ERR", "Info", "debug", "trace", "fatal", "notice", "log", "30", "50", "10", "40", "60", "[", "]", "|", " ",
	":", `"`, ",", "}", "{", "msg=x", "İ", "Ⱥ", "GET /x 200", "2026-10-11 14:04:00.123 UTC [612] ", "LOG:  ", "wrn",
	strings.Repeat("x", 50), strings.Repeat("y", 25),
}

var words = []string{"restart", "reset", "rerun", "api", "db", "wake", "worker", "task", "migrate", "Sleep", "ÉTÉ", "İstanbul", "re", "res", "r", "d"}

func join(r *rand.Rand, from []string, most int) string {
	var b strings.Builder
	for range r.IntN(most + 1) {
		b.WriteString(pick(r, from))
	}
	return b.String()
}

func stringCases(o *out, r *rand.Rand) {
	for range 2500 {
		s := join(r, pieces, 8)
		got := map[string]any{"clean": display.Clean(s)}
		if utf8.ValidString(s) {
			got["lines"], got["first_line"] = display.CleanLines(s), display.FirstLine(s)
		}
		o.emit("clean", hex.EncodeToString([]byte(s)), got)
	}
	for range 2500 {
		line := join(r, levelPieces, 7)
		rank, tag := display.Rank(line)
		o.emit("level", line, map[string]any{"level": display.Level(line), "rank": rank, "tag": tag})
	}
	for range 1500 {
		q, label := join(r, append(words, " ", " ", "\t"), 3), join(r, append(words, " ", " "), 5)
		at, ok := display.FuzzyMatch(q, label)
		o.emit("fuzzy", []string{q, label}, map[string]any{"ok": ok, "at": list(at), "score": display.FuzzyScore(at)})
	}
	for _, run := range append(runs, "docker run -it --rm -p 1:2 -e X img arg", "podman  run --platform linux/amd64 --pull=always alpine", "  docker run  ") {
		o.emit("short_run", run, display.ShortRun(run))
	}
	for _, dir := range []string{"", "/home/dev", "/home/dev/x", "/home/devx", "/srv/home/dev", "home/dev"} {
		o.emit("tilde", dir, display.Tilde(dir))
	}
	for _, e := range []string{"", "dial unix /x: connect: connection refused", "open x: no such file or directory", "unexpected EOF", "write: broken pipe", "read: connection reset by peer", "daemon not reachable", "something else"} {
		var err error
		if e != "" {
			err = errors.New(e)
		}
		o.emit("humanise", e, display.Humanise(err))
	}
	for _, id := range []string{"", "purge", "Produce", "éclair", "ßig", "ᾳx", "ǆ", "1x", "ﬀ"} {
		o.emit("action_key", id, display.ActionKey(control.ActionView{ID: id}))
	}
	for _, n := range []int{0, 1, 2, 17} {
		name := pick(r, names)
		o.emit("notices", []any{n, name}, []string{display.RunningNote(n), display.TaskStarted(name)})
	}
	for _, s := range []string{"1_000.5", "0x1p4", "0x_1.8p1", "-Infinity", "1e400", "+nan", "1__0", "_1", "0x1", "infx", "", ".", "1.e5", "1_e5", "0x1p-1074", "0x1p-1080", "0x1.fffffffffffff8p1023", "1e-400", "0x.8p1", "1_.5", "0X1P+2", "00_1", "0_x1p0", "1e+_5", "1e5_0", "nan", "+inf", "inF", "-0", "1,043", "12abc", "0x1.8", "1e", "+", "0xp1", "0x1p", "123456789012345678901234567890", "0x123456789abcdef0123p0", "4.9406564584124654e-324", "2.4703282292062327e-324"} {
		v, err := strconv.ParseFloat(s, 64)
		if err != nil {
			v = 0 // what Go returns beside an error is not part of the contract
		}
		o.emit("parse_float", s, []any{err == nil, bits(v)})
	}
	for range 300 {
		connecting, down := r.IntN(4) == 0, r.IntN(3) == 0
		retry, since := pick(r, offsets), pick(r, offsets)
		ok, text := display.Banner(connecting, down, retry, since)
		o.emit("banner", []any{connecting, down, int64(retry), int64(since)}, []any{ok, text})
	}
}
