// The weight lines CI judges on ubuntu x86_64, estimated from builds here.
//
// A macOS arm64 binary is not CI's ubuntu x86_64 binary, and the ratio between the two differs per binary and moves when profiles
// move, so each binary has its own ratio and margin, set from the same commits measured on both. Packages need no estimate:
// `cargo tree --target x86_64-unknown-linux-gnu` on the Mac lists exactly CI's packages (232, 183, 307 and 122 for main at
// db6cbc2c8, the four numbers CI printed in run 45879808204).
//
// Verdict: estimate = bytes here x ratio. FAIL when the estimate is over (line - margin): nothing here can then say CI will pass.
// WARN within one more margin below that (close; CI will very likely pass). PASS below.
//
// Evidence, 2026-11-27 (rust 1.99.1, the repo's profiles; CI's bytes from the weight and weight-doze job logs of the same commits):
//
//   binary      commit (CI run)            CI x86_64    macOS arm64   CI/mac    VM arm64 Linux   CI/VM
//   doze        4ff675326 (45732661202)    8,993,656    6,181,536     1.4549    7,542,288        1.1924
//               db6cbc2c8 (45879808204)    8,596,472    5,720,880     1.5026    7,149,072        1.2025
//   doze-kafka  4ff675326                  13,747,232   9,856,960     1.3947    11,150,384       1.2329
//               db6cbc2c8                  13,759,896   9,857,024     1.3959    11,150,384       1.2340
//   doze-aws    4ff675326                  18,837,432   12,204,720    1.5435
//               db6cbc2c8                  18,876,744   12,237,920    1.5425    15,088,312       1.2511
//   kafka lean  db6cbc2c8                  7,116,688    5,256,080     1.3540    5,775,848        1.2321
//
// Between the two commits the 2026-11-26 headroom pass moved doze-hcl, doze-config and hcl-edit to opt-level "z" and cut 397 KB
// on CI. The macOS doze ratio moved 3.3% with it (Mach-O and ELF did not shrink alike); the VM's moved 0.8%. kafka and aws moved
// at most 0.1% on macOS. So:
//
//   - doze is judged in the VM (ratio 1.2025, margin 1.8% of the line = the 0.8% spread + 1%); its macOS estimate (margin 4.3% of
//     the line) is shown but only warns, unless the VM gates cannot run, when it decides alone and can fail.
//   - doze-kafka and doze-aws are judged from macOS (margin 1.1% of the line: the 0.1% spread + 1%).
//   - kafka lean has one commit of evidence: margin 1.5% of its line, an assumption until a second commit is measured.
//
// Main itself, on these numbers: doze ~8,597,000 (VM) vs fail over 8,838,000 PASS; doze-kafka ~13,760,000 vs fail over 13,906,000
// WARN (it is 300 KB under its line); doze-aws ~18,877,000 vs 19,780,000 PASS; lean ~7,117,000 vs 7,339,000 PASS.
// Re-measure (both commits, both machines) when a profile in Cargo.toml changes or a ratio here looks off.

export interface WeightSpec {
  id: string;
  package: string;
  profile: string;
  binary: string;
  features: string[];
  /** CI bytes / macOS bytes, and the margin as a fraction of the line. */
  ratio: number;
  margin: number;
  /** CI bytes / VM bytes and margin, where the VM judges this binary. */
  vm?: { ratio: number; margin: number };
}

export const SPECS: WeightSpec[] = [
  { id: "doze", package: "doze-cli", profile: "release-doze", binary: "doze", features: [], ratio: 1.5026, margin: 0.043, vm: { ratio: 1.2025, margin: 0.018 } },
  { id: "doze-kafka", package: "doze-kafka", profile: "release", binary: "doze-kafka", features: [], ratio: 1.3959, margin: 0.011 },
  { id: "doze-aws", package: "doze-aws", profile: "release-aws", binary: "doze-aws", features: [], ratio: 1.5425, margin: 0.011 },
  { id: "kafka-lean", package: "doze-kafka", profile: "release", binary: "doze-kafka", features: ["--no-default-features"], ratio: 1.354, margin: 0.015 },
];

export type Verdict = "pass" | "warn" | "fail";

/** `margin` is a fraction of the line. */
export function judge(est: number, line: number, margin: number): Verdict {
  const m = line * margin;
  if (est > line - m) return "fail";
  if (est > line - 2 * m) return "warn";
  return "pass";
}
