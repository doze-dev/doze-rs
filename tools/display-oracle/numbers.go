package main

import (
	"math"
	"math/rand/v2"
	"time"

	"github.com/doze-dev/doze/internal/display"
)

// bits are a float's IEEE bits, every NaN written as the one Rust's f64::NAN is (a NaN's payload says nothing here).
func bits(v float64) uint64 {
	if math.IsNaN(v) {
		return 0x7ff8000000000000
	}
	return math.Float64bits(v)
}

func bitsOf(vs []float64) []uint64 {
	out := []uint64{}
	for _, v := range vs {
		out = append(out, bits(v))
	}
	return out
}

// figures sit on the edges where the formats change: rounding ties, unit boundaries, signs, and the specials.
var figures = []float64{
	0, math.Copysign(0, -1), 0.04, -0.04, 0.05, 0.25, 0.35, 0.5, 1.5, 2.5, 0.499, 0.5, 0.9949, 0.995, 1, 1.0499,
	1.05, 1.25, 22.6, 99.5, 100, 104.9, 105, 250, 1020, 12345.678, -3.5, 1e15, 1e300, math.Inf(1), math.Inf(-1),
	math.NaN(), 524287.9, 524288, 1<<30 - 1, 1 << 30, 1.5 * (1 << 30), 0.004999, 0.005, 9.95, 1e-320,
}

func figure(r *rand.Rand) float64 {
	if r.IntN(3) == 0 {
		return pick(r, figures)
	}
	return math.Round(r.Float64()*r.Float64()*float64(int64(1)<<uint(r.IntN(40)))*1000) / 1000
}

func numberCases(o *out, r *rand.Rand) {
	ints := []int64{0, 1, -1, 999, 1000, -1000, 1 << 19, 1<<20 - 1, 1 << 20, 1<<30 - 1, 1 << 30, 1288490188, math.MaxInt64, math.MinInt64}
	for range 400 {
		n := pick(r, ints)
		if r.IntN(2) == 0 {
			n = r.Int64N(1<<uint(1+r.IntN(62))) - r.Int64N(1<<20)
		}
		o.emit("int", n, map[string]any{"mb": display.MB(n), "thousands": display.Thousands(n), "plural": display.Plural(int(n))})
	}
	durs := append([]time.Duration{math.MaxInt64, 10 * 365 * 24 * time.Hour}, offsets...)
	for range 400 {
		d := pick(r, durs)
		if r.IntN(2) == 0 {
			d = time.Duration(r.Int64N(int64(1) << uint(10+r.IntN(53))))
		}
		d = max(d, 0)
		o.emit("duration", int64(d), map[string]any{"clock": display.Clock(d), "ago": display.Ago(d)})
	}
	for range 1200 {
		v, total := figure(r), figure(r)
		o.emit("float", []uint64{bits(v), bits(total)}, map[string]any{
			"cores": display.Cores(v), "cores_unit": display.CoresUnit(v), "pct": display.Pct(v), "count": display.Count(v),
			"bytes_label": bytesLabel(v), "mem_unit": memUnit(total, v),
		})
	}
	for _, n := range []int{0, 1, 59, 60, 61, 119, 125, 599, 600, 601, 3600, 3601, 7200, 172799, 172800, 1 << 30} {
		o.emit("span", n, display.SpanWord(n))
	}
}

// fits says int64(v) is the same on every platform: in range and a number.
func fits(v float64) bool { return v > -9.2e18 && v < 9.2e18 }

func bytesLabel(v float64) any {
	if !fits(v) {
		return nil
	}
	return display.BytesLabel(v)
}

func memUnit(total, v float64) any {
	if !fits(v) {
		return nil
	}
	return display.MemUnit(total)(v)
}

func seriesCases(o *out, r *rand.Rand) {
	for range 250 {
		var vs []float64
		for range r.IntN(100) {
			v := pick(r, []float64{0, 0, 1, 2.5, 9, 100, 0.1})
			if r.IntN(3) == 0 {
				v = figure(r)
			}
			if math.IsNaN(v) && r.IntN(4) > 0 {
				v = 1
			}
			vs = append(vs, v)
		}
		floor, w, zoom := pick(r, []float64{0, 1, 5, 100, -1}), r.IntN(42)-2, r.IntN(2) == 0
		var times []time.Time
		for i := range vs {
			times = append(times, now.Add(time.Duration(i)*time.Second))
		}
		if r.IntN(8) == 0 && len(times) > 0 {
			times = times[1:]
		}
		peak, mean := display.Stats(vs)
		lo, hi := display.Scale(vs, floor, zoom)
		sv, st := display.SqueezeTimed(vs, times, w)
		var stamps []int64
		for _, t := range st {
			stamps = append(stamps, t.UnixNano())
		}
		var all []display.Share
		var shareNames []string
		for i, v := range vs[:min(len(vs), 9)] {
			all = append(all, display.Share{Name: names[i], V: v})
			shareNames = append(shareNames, names[i])
		}
		shares, idle, sum := display.Shares(all, floor)
		var drawn [][]any
		for _, s := range shares {
			drawn = append(drawn, []any{s.Name, bits(s.V)})
		}
		inner := r.IntN(80)
		// int(x/sum*inner) of a non-finite share is the platform's choice: leave those out
		var cells any = list(display.ShareCells(shares, sum, inner))
		for _, s := range shares {
			if math.IsInf(s.V, 0) || math.IsInf(sum, 0) {
				cells = nil
			}
		}
		from := len(vs) - len(times)
		o.emit("series", map[string]any{"vs": bitsOf(vs), "floor": bits(floor), "w": w, "zoom": zoom, "times": len(times), "times_from": from, "inner": inner, "names": list(shareNames)}, map[string]any{
			"stats": bitsOf([]float64{peak, mean}), "scale": bitsOf([]float64{lo, hi}), "squeeze": bitsOf(display.Squeeze(vs, w)),
			"squeeze_timed": []any{bitsOf(sv), list(stamps)}, "shares": list(drawn), "idle": idle, "sum": bits(sum),
			"cells": cells,
		})
	}
}
