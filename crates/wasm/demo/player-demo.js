import init, { WasmPlayer, GenerativeCanvas } from '../pkg/rust_motion_wasm.js';

let player = null;
let generativeCanvas = null;
let isPlaying = false;
let loopEnabled = true;

// DOM Elements
const canvas = document.getElementById('rust-motion-canvas');
const scrubber = document.getElementById('timeline-scrubber');
const btnPlayPause = document.getElementById('btn-play-pause');
const btnStepPrev = document.getElementById('btn-step-prev');
const btnStepNext = document.getElementById('btn-step-next');
const btnLoop = document.getElementById('btn-loop');
const timecodeDisplay = document.getElementById('timecode-display');
const frameDisplay = document.getElementById('frame-display');
const speedSelect = document.getElementById('speed-select');
const fpsSelect = document.getElementById('fps-select');
const compositionSelect = document.getElementById('composition-select');
const propsEditor = document.getElementById('props-editor');
const btnApplyProps = document.getElementById('btn-apply-props');
const engineStatus = document.getElementById('engine-status');
const hudComposition = document.getElementById('hud-composition');
const hudStats = document.getElementById('hud-stats');
const metricLatency = document.getElementById('metric-latency');
const metricFps = document.getElementById('metric-fps');

function updateUI() {
  if (!player) return;

  const currentFrame = player.get_current_frame();
  const duration = player.get_duration_in_frames();
  const timecode = player.get_timecode();
  const latency = player.get_last_render_time_ms();
  const compId = player.get_composition();

  scrubber.value = currentFrame;
  scrubber.max = Math.max(0, duration - 1);
  frameDisplay.textContent = `Frame ${currentFrame} / ${duration}`;
  timecodeDisplay.textContent = timecode;
  hudComposition.textContent = `Comp: ${compId}`;

  const fpsCalc = latency > 0 ? Math.round(1000 / latency) : 1000;
  hudStats.textContent = `Latency: ${latency.toFixed(1)} ms | ~${fpsCalc} FPS`;
  metricLatency.textContent = `${latency.toFixed(1)} ms`;
  metricFps.textContent = `~${fpsCalc} FPS`;
}

function loop(timestamp) {
  if (player) {
    if (player.is_playing()) {
      player.tick(timestamp);
      updateUI();
    }
  }

  // Animate the TouchDesigner Generative Canvas independently at 60 FPS
  if (generativeCanvas) {
    const t = timestamp * 0.001;
    generativeCanvas.render_noise(
      t,
      3.5, // scale
      4,   // octaves
      14, 165, 233, // Teal/Sky color A
      168, 85, 247  // Purple color B
    );
  }

  requestAnimationFrame(loop);
}

async function main() {
  try {
    engineStatus.textContent = 'Loading WebAssembly binary...';
    await init();
    engineStatus.textContent = 'Engine: WASM Active (Ready)';

    // Instantiate WasmPlayer: 960x540, 30fps, 150 frames
    player = new WasmPlayer('rust-motion-canvas', 960, 540, 30.0, 150);

    // Instantiate TouchDesigner-style GenerativeCanvas: 320x180
    generativeCanvas = new GenerativeCanvas('generative-canvas', 320, 180);

    // Populate composition select from registry
    const availableComps = player.get_available_compositions();
    compositionSelect.innerHTML = '';
    for (const comp of availableComps) {
      const opt = document.createElement('option');
      opt.value = comp;
      opt.textContent = comp === 'TouchDesignerGenerative'
        ? `${comp} (TouchDesigner TOP)`
        : comp;
      compositionSelect.appendChild(opt);
    }
    compositionSelect.value = player.get_composition();

    // Hook Scrubber input for 0ms latency seeking
    scrubber.addEventListener('input', (e) => {
      const targetFrame = Number(e.target.value);
      player.seek(targetFrame);
      updateUI();
    });

    // Play / Pause
    btnPlayPause.addEventListener('click', () => {
      isPlaying = player.toggle();
      btnPlayPause.textContent = isPlaying ? '⏸ Pause' : '▶ Play';
      if (isPlaying) {
        btnPlayPause.classList.remove('btn-primary');
        btnPlayPause.style.background = '#e11d48';
      } else {
        btnPlayPause.classList.add('btn-primary');
        btnPlayPause.style.background = '';
      }
    });

    // Step +/- 1 frame
    btnStepPrev.addEventListener('click', () => {
      player.pause();
      isPlaying = false;
      btnPlayPause.textContent = '▶ Play';
      const prev = Math.max(0, player.get_current_frame() - 1);
      player.seek(prev);
      updateUI();
    });

    btnStepNext.addEventListener('click', () => {
      player.pause();
      isPlaying = false;
      btnPlayPause.textContent = '▶ Play';
      const next = Math.min(player.get_duration_in_frames() - 1, player.get_current_frame() + 1);
      player.seek(next);
      updateUI();
    });

    // Loop toggle
    btnLoop.addEventListener('click', () => {
      loopEnabled = !loopEnabled;
      player.set_loop(loopEnabled);
      btnLoop.textContent = loopEnabled ? '🔁 Loop: ON' : '🔁 Loop: OFF';
      btnLoop.style.borderColor = loopEnabled ? 'var(--accent)' : 'var(--border)';
    });

    // Playback Speed
    speedSelect.addEventListener('change', (e) => {
      player.set_playback_rate(parseFloat(e.target.value));
    });

    // FPS
    fpsSelect.addEventListener('change', (e) => {
      player.set_fps(parseFloat(e.target.value));
      updateUI();
    });

    // Composition Switch
    compositionSelect.addEventListener('change', (e) => {
      try {
        player.set_composition(e.target.value);
        updateUI();
      } catch (err) {
        console.error('Failed to set composition:', err);
      }
    });

    // Dynamic Props
    btnApplyProps.addEventListener('click', () => {
      try {
        player.set_props(propsEditor.value);
        updateUI();
      } catch (err) {
        alert('Invalid JSON: ' + err);
      }
    });

    updateUI();
    requestAnimationFrame(loop);
  } catch (err) {
    console.error('WASM initialization failed:', err);
    engineStatus.textContent = 'Engine: Error (' + err + ')';
    engineStatus.style.color = '#f87171';
  }
}

main();
