#!/usr/bin/env python3
"""Compare release Rust with the actual vendored TS, including its warm caches."""
import argparse
import json
import hashlib
import platform
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--repeats', type=int, default=5)
args = parser.parse_args()
subprocess.run(['cargo', 'build', '--locked', '--release', '-p', 'rust-motion-animation', '--example', 'spring_bench'], cwd=ROOT, check=True)
results = []
for scenario in ['cold', 'repeat', 'sequential', 'duration', 'seek', 'measure', 'varied']:
    count, warmup = (300, 0) if scenario == 'cold' else (100000, 10000)
    samples = {'rust-motion': [], 'remotion': []}
    checksums = {'rust-motion': [], 'remotion': []}
    for repeat in range(args.repeats):
        for engine in (['rust-motion', 'remotion'] if repeat % 2 == 0 else ['remotion', 'rust-motion']):
            command = [str(ROOT / 'target/release/examples/spring_bench')] if engine == 'rust-motion' else ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', str(ROOT / 'benchmarks/spring-remotion.mjs')]
            sample = json.loads(subprocess.check_output(command + [scenario, str(count), str(warmup)], cwd=ROOT, text=True))
            samples[engine].append(sample['ns_per_call'])
            checksums[engine].append(sample['checksum'])
    difference = max(abs(a-b) for a,b in zip(checksums['rust-motion'], checksums['remotion']))
    if difference > 1e-6:
        raise RuntimeError(f'{scenario}: checksum mismatch {difference}')
    rust = statistics.median(samples['rust-motion'])
    ts = statistics.median(samples['remotion'])
    result = dict(scenario=scenario, calls=count, warmup_calls=warmup, samples_ns=samples, median_ns=dict(rust-motion=rust, remotion=ts), speedup=ts/rust, checksum_max_difference=difference)
    results.append(result)
    print(f'{scenario:12} Rust {rust:10.1f} ns | Remotion {ts:10.1f} ns | {ts/rust:.2f}x', flush=True)
report = dict(spring_source_sha256=hashlib.sha256((ROOT / 'crates/animation/src/spring.rs').read_bytes()).hexdigest(), scope='spring kernel only; excludes process startup/import/build', platform=platform.platform(), node=subprocess.check_output(['node','--version'], text=True).strip(), rust=subprocess.check_output(['rustc','--version'], text=True).strip(), repeats=args.repeats, results=results)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, indent=2) + '\n')

if any(row['speedup'] <= 1.0 for row in results):
    raise SystemExit('Performance gate failed: at least one workload did not beat Remotion; see saved report')
