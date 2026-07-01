# glow-bloom

GPU-accelerated glow/bloom text rendering for egui.

## Usage

```rust
// 1. Create the renderer with a glow context and font bytes
let gl = cc.gl.as_ref().expect("Need glow backend");
let font = include_bytes!("../assets/Courier 10 Pitch Bold.otf").to_vec();
let renderer = Arc::new(egui::mutex::Mutex::new(
    glow_bloom::BloomRenderer::new(gl, font),
));

// 2. Use the egui widget
ui.add(
    glow_bloom::BloomText::new(Arc::clone(&renderer), "Hello, world!")
        .intensity(1.8)
        .font_scale(72.0),
);

// 3. Clean up on exit
renderer.lock().destroy(gl);
```

## Examples

```sh
cargo run --example simple
cargo run --example typewriter
```
