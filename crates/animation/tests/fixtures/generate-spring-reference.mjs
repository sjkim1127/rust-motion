import {registerHooks} from 'node:module';
import {existsSync, writeFileSync} from 'node:fs';
import {fileURLToPath, pathToFileURL} from 'node:url';
const root = pathToFileURL(process.cwd() + '/vendor/remotion-4.0.495/packages/core/src/');
registerHooks({resolve(specifier, context, nextResolve) {
  if (specifier.startsWith('.') && context.parentURL?.startsWith(root.href)) {
    const url = new URL(specifier, context.parentURL);
    const path = fileURLToPath(url).replace(/\.js$/, '.ts');
    if (existsSync(path)) return nextResolve(pathToFileURL(path).href, context);
    if (existsSync(path + '.ts')) return nextResolve(pathToFileURL(path + '.ts').href, context);
  }
  return nextResolve(specifier, context);
}});
const {spring, measureSpring} = await import(new URL('spring/index.ts', root));
const configs = [{}, {damping: 200}, {damping: 6, mass: 2, stiffness: 80}, {overshootClamping: true}];
const options = [{}, {durationInFrames: 10}, {durationInFrames: 45, delay: 12.5}, {reverse: true}, {durationInFrames: 10, delay: 5, reverse: true}, {from: -50, to: 200}, {durationInFrames: 20, durationRestThreshold: 0.001}];
const rows = [];
for (const fps of [15, 30, 59.94]) for (let ci = 0; ci < configs.length; ci++) for (let oi = 0; oi < options.length; oi++) {
  // Preserve Rust Motion's range clamping contract rather than upstream's non-unit-range clamp bug.
  if (ci === 3 && oi === 5) continue;
  for (const frame of [-2, 0, 0.5, 1, 5, 9.5, 10, 15, 28, 60]) {
    const c = configs[ci], o = options[oi];
    rows.push([fps, c.damping ?? 10, c.mass ?? 1, c.stiffness ?? 100, c.overshootClamping ?? false, frame, o.from ?? 0, o.to ?? 1, o.durationInFrames ?? '', o.durationRestThreshold ?? 0.005, o.delay ?? 0, o.reverse ?? false, spring({fps, frame, config:c, ...o})].join('\t'));
  }
}
writeFileSync('crates/animation/tests/fixtures/remotion_spring.tsv', '# Generated from vendor/remotion-4.0.495 spring/index.ts with Node 24\n# fps damping mass stiffness clamp frame from to duration threshold delay reverse expected\n' + rows.join('\n') + '\n');
const measurements = [];
for (const fps of [15,30,59.94]) for (const config of configs) for (const threshold of [0.001, 0.005, 0.05, 1]) measurements.push([fps, config.damping ?? 10, config.mass ?? 1, config.stiffness ?? 100, config.overshootClamping ?? false, threshold, measureSpring({fps,config,threshold})].join('\t'));
writeFileSync('crates/animation/tests/fixtures/remotion_measure_spring.tsv', '# Generated from vendor/remotion-4.0.495 spring/measure-spring.ts\n# fps damping mass stiffness clamp threshold expected\n' + measurements.join('\n') + '\n');
console.log(`Generated ${rows.length} spring samples and ${measurements.length} durations from vendor source`);
