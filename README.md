# Ukeela

Ukeela is a learning project for exploring computer-vision concepts from OpenCV and Rust equivalents. Video stabilization is the practical focus: it provides a way to experiment with feature detection, optical-flow tracking, motion estimation, and smoothing while building a video-processing pipeline in Rust.

The workspace is split into crates for shared frame and middleware types, processing chains, the frame pipeline, video sources, and individual processing steps. The CLI connects these pieces; it is an example and control surface for the project, rather than the project's primary goal.

## Prerequisites

Install Rust on Linux/MacOS with:

```bash
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
```

If you need install on Windows visit [this](https://rust-lang.org/tools/install/) page.

## Running

The following example reads from a camera, displays the stabilized video(without cropping) with FPS and feature overlays, and enables the Lucas–Kanade optical-flow path:

```sh
RUSTFLAGS="-C target-cpu=native" cargo run --release -- \
  --scale 100 camera /dev/video0 display \
  --log-level info \
  --features-method fast \
  --show-fps \
  --display-features \
  --display-motion-vectors \
  --smoothing-factor 10 \
  --display-stabilization \
  --processing-type single \
  --optical-flow-lk
```

Use `cargo run -p ukeela-cli -- --help` to see all available sources, outputs, and processing options.
