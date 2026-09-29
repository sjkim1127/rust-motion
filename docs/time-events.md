# Named time events

Projects can store named frame markers and use them as stable timing anchors in
compositions. The Studio event lane lets you move a marker; by default, markers
at that frame and later frames move together. Hold Alt (Option on macOS) while
dragging to move only the selected marker. Save the project to persist edits.

Open [`examples/named-time-events.dioxuscut.json`](../examples/named-time-events.dioxuscut.json)
in Studio to see the `NamedTimeEventsDemo`. Its progress bar waits for
`voice_start` and derives its duration from `voice_start` to `voice_end`.
Moving either marker changes the preview and the rendered frames.

## Project format

`events` is an optional array of `{ "id", "frame" }` records. Event IDs must
be non-empty and unique; frames use the project timeline and must be inside its
duration. The optional `voice_over_asset_id` points to an audio asset in the
project's `assets` list. Studio can import a voice-over file or select an audio
asset already in the project, then audition it while playing or scrubbing the
timeline. Project audio assets are also available to the existing render audio
pipeline.

Older project files without these fields remain valid. Saving a project with
events writes the fields into the regular project JSON, so reopening preserves
the event names and positions.

## Rust compositions

`NativeComposition::render_with_time_events` receives the immutable schedule
prepared for the render job. Existing compositions can keep implementing
`render`; the default adapter preserves their behavior. For example:

```rust,ignore
fn render_with_time_events(
    &self,
    frame: u32,
    _props: &serde_json::Value,
    _context: NativeCompositionContext,
    events: &TimeEventSchedule,
) -> Result<Scene, CompositionError> {
    let start = events
        .wait_until("voice_start")
        .map_err(|error| CompositionError::render(frame, error.to_string()))?;
    let duration = events
        .duration_between("voice_start", "voice_end")
        .map_err(|error| CompositionError::render(frame, error.to_string()))?;
    let progress = (frame.saturating_sub(start) as f32 / duration.max(1) as f32)
        .clamp(0.0, 1.0);
    let mut scene = Scene::new();
    scene.push(SceneNode::Rect {
        x: 0.0,
        y: 0.0,
        w: 1000.0 * progress,
        h: 80.0,
        fill: Color::rgb(249, 115, 62),
        stroke: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
    });
    Ok(scene)
}
```

`wait_until` returns the event's absolute frame in the composition's schedule.
`duration_between` returns the non-negative frame distance between two events.
For a clip on a project track, Rust and browser compositions receive events
inside that clip translated to clip-local frames. The schedule is prepared
before frame rendering and is immutable, so rendering frames in a different
order or concurrently gives the same event positions. A marker exactly at a
clip's end is available as its terminal point, which lets a composition use
that marker as the end of a duration interval.

If a composition asks for a removed or renamed event, the render reports a
clear missing-event error containing its name. Reusing an event name for a
different meaning should be treated as a composition change.

## Browser compositions

Studio and the browser worker expose the same helpers on `window.dioxuscut`:

```js
const start = window.dioxuscut.waitUntil('voice_start');
const duration = window.dioxuscut.getTimeEventDuration('voice_start', 'voice_end');
const progress = Math.max(0, Math.min(1, (frame - start) / Math.max(1, duration)));
```

Browser compositions registered with `registerComposition` or
`registerThreeComposition` also receive a `timeEvents` array in the render
context. Each entry has the event `id` and local `frame`. Missing event lookups
throw an error naming the missing marker.
