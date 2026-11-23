
─── ukeela-common/src/extensions/lock.rs:5-5 ───
[documentation · low] These public traits (`AcquireRwLock`, `AcquireMutexLock`) and their methods
have no documentation, and neither the traits nor the blanket impls state the panic-on-poison
contract. Since this is a public crate-boundary API consumed by other crates, add doc comments that
describe intent and explicitly document the panic condition (or remove the panic as noted above).



─── ukeela-common/src/types/mod.rs:3-3 ───
[maintainability · low] This module is publicly re-exported (`pub mod types;` in lib.rs) and uses a
glob re-export. Because `percent` is a private module, any item added to it later becomes part of
the public API of `types` implicitly, and adding a symbol that collides with an existing one (or
with a future `types` item) is a silent breaking change rather than a compile error. Prefer explicit
re-exports so the public surface is intentional and documented at the crate boundary.

- pub use self::percent::*;
+ pub use self::percent::Percent;


─── ukeela-common/src/frame.rs:43-43 ───
[documentation · low] Spelling/formatting error in a user-facing error message: "unknow" should be
"unknown", and the padding before the colon is inconsistent with other messages in this file.

-             _ => return Err(format!("unknow video format : {s}")),
+             _ => return Err(format!("unknown video format: {s}")),


─── ukeela-common/src/middleware.rs:34-35 ───
[documentation · low] `define_middleware_dispatch` is `#[macro_export]`ed public API but has no
documentation. Since the expansion is not visible to callers, please document what it generates (an
enum implementing `$crate::Middleware` by dispatching `name`/`process`/`process_async` to the inner
value) and its constraint that every variant must be a single-field tuple variant whose type
implements `Middleware`; otherwise the failure mode for an unsupported variant shape is just an
opaque macro-matching error.



─── ukeela-middleware/ukeela-middleware-grayscale/src/lib.rs:37-37 ───
[maintainability · low] Formatting the `purecv` error into a string discards its type and source
chain, so callers cannot inspect the underlying error and the message carries no context about which
step failed. Prefer preserving the error with `anyhow::Error::from`/`.context(...)` (or at minimum
prefix it with context, e.g. `"grayscale conversion failed: {e}"`).

-         .map_err(|e| anyhow::anyhow!("{e}"))?;
+         .map_err(|e| anyhow::anyhow!("grayscale conversion failed: {e}"))?;


─── ukeela-middleware/ukeela-middleware-optical-flow-estimation/src/lib.rs:41-42 ───
[maintainability · low] Inconsistent clearing semantics: when `matches.len() < min_matches` the
function returns early and leaves any pre-existing `similarity_transform` untouched, but here a
failed estimation (non-finite matches, RANSAC failure) overwrites it with `None`, discarding a
transform populated upstream. Either clear in both paths or keep the previous value in both;
document the chosen behaviour so downstream consumers can rely on it.



─── ukeela-middleware/ukeela-middleware-optical-flow-estimation/src/lib.rs:96-99 ───
[performance · low] `RANSAC_THRESHOLD.powi(2)` is recomputed for every match in every one of the
2000 iterations (twice, counting the refinement pass). Hoist it into a constant so the inner loop
only performs the comparison.

          let inlier_count = valid_matches
              .iter()
-             .filter(|m| reprojection_error_squared(candidate, **m) <= RANSAC_THRESHOLD.powi(2))
+             .filter(|m| reprojection_error_squared(candidate, **m) <= RANSAC_THRESHOLD_SQUARED)
              .count();


─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/lib.rs:25-28 ───
[documentation · low] `FeatureDetectionConfig` is part of the crate's public surface but neither the
struct nor its fields are documented, and the meaning/units of `distance_to_border` (pixels from
each border, and how it interacts with the detector's own min-distance) is not obvious to callers.
Since this is a config type at the crate boundary, add doc comments describing each field and the
valid ranges.



─── ukeela-cli/src/args.rs:22-24 ───
[maintainability · low] `derive_more::Display` renaming and clap's `ValueEnum` naming are out of
sync for multi-word variants: `#[display(rename_all = "lowercase")]` produces `catmullrom`, while
clap derives the accepted CLI value `catmull-rom` from `CatmullRom`. So a value shown/logged as
`catmullrom` is rejected by the parser. This also makes the `default_value_t` path
(`Display::to_string`) unsafe for any future multi-word default. Align the two by telling clap to
use lowercase names, e.g. annotate the enum with `#[value(rename_all = "lower")]`.

  #[derive(Default, Clone, Copy, Debug, PartialEq, Eq, derive_more::Display, clap::ValueEnum)]
  #[display(rename_all = "lowercase")]
+ #[value(rename_all = "lower")]
  pub enum FilterType {


─── ukeela-cli/src/args.rs:52-52 ───
[documentation · low] Spelling: `it's` should be `its` (possessive, not a contraction). This doc
comment is rendered as user-facing `--help` text.

-     /// Scale original image to specific percent of it's original size
+     /// Scale original image to specific percent of its original size


─── ukeela-cli/src/args.rs:77-78 ───
[documentation · low] `num_features`, `distance_to_border`, `fast_threshold` and
`motion_min_matches` have no doc comments, so clap shows no descriptions for them in `--help` while
the other processing options do. Add short `///` docs describing units/ranges (e.g.
`distance_to_border` in pixels, `fast_threshold` on the 0-255 FAST scale).



─── ukeela-cli/src/args.rs:89-93 ───
[bug · low] fps/width/height are plain u32 in both CLI args and GStreamer configs, so 0 (and
absurdly large values) are representable and interpolated into caps strings (framerate={fps}/1,
width={width},height={height}), where framerate=0/1 or zero dimensions are invalid and only surface
as opaque GStreamer pipeline errors. Add lower-bound validation (clap value_parser range 1.. for
fps/width/height) and/or model the source config fields with NonZeroU32 so invalid configs cannot be
constructed.



─── ukeela-cli/src/bin/main.rs:21-22 ───
[bug · low] `unwrap_or_default()` silently maps a missing home directory (`HOME` unset, e.g. in a
service/container) to an empty path, so the default becomes the relative path `logs` and logs are
written into the current working directory instead of an absolute, predictable location. Use an
absolute fallback (e.g. the temp dir) or fail clearly so log placement is deterministic.

-     #[arg(long, required = false, default_value_os_t = dirs::home_dir().unwrap_or_default().join("logs") )]
+     #[arg(long, required = false, default_value_os_t = dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("logs"))]
      pub log_output_dir: PathBuf,


─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/src/lib.rs:6-7 ───
[documentation · low] The crate root has no crate-level documentation while exposing two public
middleware modules. Consider a `//!` block stating what the crate does (draws green feature-point
rings / red match arrows on RGBA frames), which modules are useful, and whether the consumers should
go through `feature_detection::FeaturePointOverlayMiddleware` and
`motion_tracking::MotionMatchesOverlayMiddleware`, so the public API surface is discoverable at the
crate boundary.



─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/fast.rs:25-25 ───
[documentation · low] The error message contains a grammatical error that hurts readability of a
public diagnostic: "don't contains" should be "doesn't contain". (The sibling LK implementation
already uses the correct wording.)

-             return Err(anyhow::anyhow!("frame metadata don't contains grayscale"));
+             return Err(anyhow::anyhow!("frame metadata doesn't contain grayscale"));


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/fast.rs:31-32 ───
[maintainability · low] The is_none() check followed by as_ref().expect(...) is a redundant
invariant assertion in library/production code and the panic message reads poorly ('we checked that
it some'); the adjacent grayscale error also has bad wording. Collapse this into a single
ok_or_else/anyhow error so the expect disappears and the error is reported once with clear context.

              .as_ref()
-             .expect("we checked that it some");
+             .ok_or_else(|| anyhow::anyhow!("frame metadata doesn't contain grayscale"))?;


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/fast.rs:36-42 ───
[performance · low] Sorting all detected keypoints just to keep the top `max_features` is O(n log n)
on a per-frame hot path. A full sort is unnecessary when `max_features` is typically much smaller
than the number of detections; a partial selection is O(n).

-         points.sort_by(|a, b| {
+         if points.len() > self.max_features {
+             points.select_nth_unstable_by(self.max_features, |a, b| {
-             b.response
+                 b.response
-                 .partial_cmp(&a.response)
+                     .partial_cmp(&a.response)
-                 .unwrap_or(std::cmp::Ordering::Equal)
+                     .unwrap_or(std::cmp::Ordering::Equal)
-         });
+             });
- 
-         points.truncate(self.max_features);
+             points.truncate(self.max_features);
+         }


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/mod.rs:9-9 ───
[bug · low] distance_to_border is declared as u32 but consumed as a signed edge threshold via as i32
in orb.rs. Values above i32::MAX silently wrap to a negative threshold (also triggering
cast_possible_wrap), producing a bogus detector configuration instead of an error. Use a type that
cannot overflow the consumer (i32/u16) or a checked/saturating conversion, ideally validated in a
constructor.



─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/feature_detection.rs:41-42 ───
[maintainability · low] `7.0` (keypoint size) and `-1.0` (angle) are magic numbers tied to the
detector/descriptor contract that will be consumed downstream. If the tracker or any later stage
relies on the keypoint size, this is an easy place for the two to silently diverge. Consider a named
constant (or a `FeatureDetectionConfig` field) so the intent and value are documented instead of an
inline literal.



─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/motion_tracking.rs:35-37 ───
[maintainability · low] Unlike the missing-grayscale case above, an empty `feature_points` list is
accepted silently: if the feature-detection middleware is not placed before this one in the chain
(or emits nothing), this middleware will quietly produce empty `motion_matches` for every frame, and
the failure only shows up as an empty estimation result downstream. Consider surfacing this as an
actionable error/warning (e.g. when the previous frame is available but carried zero feature
points), so chain misconfiguration is diagnosed at the boundary.



─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/motion_tracking.rs:45-47 ───
[bug · low] `acquire_lock` in `ukeela_common::extensions::lock` uses `lock().expect("poisoned
lock")`, and the guard is held across the foreign tracker call. If `advance`/`track_fb` panics on
any frame (index/length assumptions inside the `optical_flow_lk` crate on arbitrary video input are
plausible), the shared `TrackerState` is permanently poisoned: every subsequent frame, including
those of every `Clone` of this middleware, panics/errors via `JoinError` and the middleware can
never recover. Additionally, `mem::take` clears `previous_points` *before* the tracking work, so a
panic leaves the state partially updated. Consider avoiding the destructive take until tracking
succeeds (e.g. track against a borrow, then swap) and/or making the lock acquisition poison-tolerant
so one bad frame does not disable the whole middleware.



─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/orb.rs:55-55 ───
[performance · low] Every call to `process_async` clones the whole middleware (and therefore the
`Orb` detector) just to obtain a `'static` value for `spawn_blocking`. Other middlewares in this
workspace keep their detector/state behind `Arc` (e.g. resize, stabilization, pcv motion_tracking)
so the per-frame clone is cheap; if `Orb` ever starts holding pattern tables or scratch buffers,
this becomes a per-frame allocation cost. Consider storing the detector as `Arc<Orb>` and cloning
the `Arc` here.



─── ukeela-processing-chain/src/lib.rs:6-6 ───
[style · low] `Middleware` is already declared as `pub trait Middleware: Send + Sync`
(ukeela-common/src/middleware.rs:6), so repeating `Send + Sync` here is redundant and easy to get
out of sync if the supertraits change. Also, this is the public API surface of the crate and has no
trait-level docs: the semantics of `push`/`pop` (dropped frame on overwrite, cancellation-safety of
`pop`) are not expressed anywhere, which makes correct use of the trait hard to infer.

Suggested: keep only `Middleware + 'static` and document the trait contract.

- pub trait ProcessingChain<T: Middleware + Send + Sync + 'static> {
+ /// Chain of middlewares processing `Frame`s.
+ ///
+ /// `push` enqueues/replaces a frame and returns any displaced frame, `pop` awaits the
+ /// next processing result and applies all added middlewares in order.
+ pub trait ProcessingChain<T: Middleware + 'static> {


─── ukeela-processing-chain/src/lib.rs:7-7 ───
[maintainability · low] `new()` returning `Self` without a `where Self: Sized` bound is problematic
for a trait: it contributes to the trait being non-dyn-compatible and cannot be called on any erased
form of the chain. The idiomatic alternative is to require `Default` (or a constructor that takes
the middlewares) instead of baking a `new` into the trait.

-     fn new() -> Self;
+     fn new() -> Self
+     where
+         Self: Sized;


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/motion_tracking.rs:91-91 ───
[bug · low] The tracking state is only advanced when `calc_optical_flow_pyramid_lk` succeeds,
because the `?` above returns early on error. After a failed frame,
`previous_gray`/`previous_points` still point at frame N-1 while the next successful frame is N+1,
so the produced `MotionMatch` values describe motion across two frame intervals without any
indication to downstream consumers. Consider resetting the state (or advancing it) in the error path
so a stale reference frame is never reused, e.g. clear `previous_gray`/`previous_points` when
tracking fails.



─── ukeela-processing-chain/src/single.rs:6-7 ───
[maintainability · low] Deriving `Default` on a generic struct adds a spurious `T: Default` bound,
so `SingleProcessingChain::<SomeMw>::default()` only compiles when the middleware type is itself
`Default`. Since `new()` (the trait constructor) deliberately has no such bound, the derived impl is
inconsistent with it and will surprise callers. Either implement `Default` manually (delegating to
`new()`) or drop the derive.

- #[derive(Debug, Default)]
+ #[derive(Debug)]
  pub struct SingleProcessingChain<T> {


─── ukeela-source/ukeela-source-gstreamer/src/lib.rs:17-17 ───
[maintainability · low] `Display {}` is an empty braced variants with no fields, which forces every
consumer to write `GstOutputConfig::Display { .. }` (see output.rs) for no benefit and is
inconsistent with the sibling unit variant `FakeSink`. Prefer a plain unit variant unless a field is
planned.

-     Display {},
+     Display,


─── ukeela-processing-chain/src/multi.rs:30-30 ───
[style · low] Typos/quality in the panic message (`"erro"` -> `"error"`), and the two `TrySendError`
cases are conflated: `into_inner()` returns the `Ok` payload for both `Full` and `Closed`, so a
closed channel is indistinguishable from a full one for the caller. The `expect` is provably
infallible, so the message is only ever seen if the wrapping invariant is broken; make it state the
invariant clearly.

-             Err(err) => Some(err.into_inner().expect("we wrapped frame into erro")),
+             Err(err) => Some(
+                 err.into_inner()
+                     .expect("frame is always wrapped in Ok before being sent"),
+             ),


─── ukeela-pipeline/src/lib.rs:118-118 ───
[bug · low] `avg_fps` is computed from the cumulative `frames_processed` and the total elapsed time
since the pipeline started, so it is a lifetime average (diluted by idle periods) rather than the
throughput for the reported interval, despite the log message implying a current rate. It is also
stored as `u8`, so any rate above 255 fps silently saturates at 255. Track frames processed since
the previous tick and divide by the tick interval, and widen the type to `u16`/`u32` or a float.



─── ukeela-source/ukeela-source-gstreamer/src/input.rs:49-49 ───
[style · low] Typo in the log message: 'pipine' should be 'pipeline'.

-         tracing::info!(pipeline = %pipeline, "Opening GStreamer Ingress pipine");
+         tracing::info!(pipeline = %pipeline, "Opening GStreamer Ingress pipeline");


─── ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs:69-71 ───
[performance · low] The whole full-resolution frame is warped and then the crop is copied again by
`crop_imm(...).to_image()`, so every frame allocates and touches roughly twice the output size (for
4K RGBA that is ~33 MB warped + ~30 MB cropped) even though most of the warped pixels are
immediately discarded. Consider warping only the region that survives the crop (pre-cropping the
source with an offsetted translation in the projection) or reusing a preallocated output buffer.



─── Cargo.toml:20-20 ───
[maintainability · low] Package/release metadata is incomplete across the workspace:
`[workspace.package]` only defines `license`, so member crates inherit neither `description` nor
`repository`; several internal/binary crates (`ukeela-cli`, middleware/source crates, and the
`ukeela-source-cam` stub) also do not set `publish = false` and have path-only dependencies, so an
accidental `cargo publish` would fail late or push an incomplete crate. Add
`description`/`repository` (and `rust-version` where appropriate) to `[workspace.package]` and
inherit them via `.workspace = true`; for crates that are not meant to be published, explicitly set
`publish = false`. If a crate is publishable, also add `version` to workspace path dependencies and
move hard-coded `version` fields to `[workspace.package]` with `version.workspace = true` for
consistency.

  license = "MIT"
+ repository = "<repo-url>"
+ description = "Ukeela video processing pipeline"


─── ukeela-cli/Cargo.toml:3-3 ───
[maintainability · low] `version` is hard-coded in member manifests while `license`/`edition` are
inherited from `[workspace.package]`. Move `version` into `[workspace.package]` and use
`version.workspace = true` to avoid drift across crates.

- version = "0.1.0"
+ version.workspace = true


─── Cargo.toml:16-16 ───
[maintainability · low] `ukeela-source-cam` is a workspace member but, unlike every other internal
crate, has no entry in `[workspace.dependencies]` (compare `ukeela-source-gstreamer.path = ...`).
Either register it there for consistency (so consumers can use `ukeela-source-cam.workspace = true`)
or drop the empty stub member until it is actually implemented.



─── Cargo.toml:31-31 ───
[maintainability · low] Commented-out dependency left in the manifest. No member references
`thiserror.workspace = true`, so this line is dead configuration that will rot; remove it (or
re-enable it properly) to keep the workspace dependencies list the single source of truth.



─── ukeela-source/ukeela-source-cam/Cargo.toml:7-7 ───
[maintainability · low] The `[dependencies]` table is empty, so it adds nothing to the manifest.
Drop it until real dependencies are added (and, when they are, declare them via `foo.workspace =
true` so workspace-managed versions/features stay the single source of truth, as the sibling
`ukeela-source-gstreamer` manifest does).



─── .gitignore:3-5 ───
[maintainability · low] `.bin`, `.build` and `build` are unanchored patterns, so they ignore any
directory with that name at any depth (e.g. a legitimate `src/build` or a nested `.bin` of a
vendored dependency), not just the project-local build output. If these are meant to be output
directories at the repository root, anchor them with a leading `/`. In addition, common generated
artifacts/logs (`*.log`, `logs/`, `coverage/`, `dist/`, `node_modules/`, `*.class`, `*.iml`,
`.vscode/`, `.settings/`, `.classpath`, `.project`) are not covered, so IDE/build noise is likely to
be committed.

- .bin
- .build
- build
+ /.bin
+ /.build
+ /build
+ 
+ *.log
+ logs/
+ coverage/
+ dist/
+ node_modules/
+ *.class
+ *.iml
+ .vscode/
+ .settings/
+ .classpath
+ .project


─── ukeela-common/src/extensions/lock.rs:16-16 ───
[bug · medium] Panicking on a poisoned lock makes poisoning permanent and cascading: if any thread
panics while holding one of these locks, the lock stays poisoned forever, so every subsequent
`acquire_*` call (e.g. in the middleware `state.acquire_lock()` processing paths) panics again and
the pipeline can never recover. Poisoning is a recoverable condition — prefer recovering the inner
guard (`PoisonError::into_inner`) when the protected state is still usable, or expose a `try_*`
variant returning a typed error so callers can decide. At minimum, document the panic behavior of
these trait methods.

- self.read().expect(POISONED_LOCK_MSG)
+         self.read().unwrap_or_else(std::sync::PoisonError::into_inner)


─── ukeela-common/src/types/percent.rs:6-6 ───
[maintainability · medium] Two API-design problems here:
1. The type is named `Percent` but stores a 0..=1 *fraction*, not 0..=100. Every current call site
has to divide by 100 (`f32::from(scale) / 100.0`), which is exactly the kind of conversion that
eventually gets forgotten and produces 100x bugs. Rename the type (e.g. `Ratio`/`Fraction`) or
document the unit explicitly on the type and the parameter.
2. `scale <= 0.0` is rejected without any documentation, so legitimate no-op values are
unrepresentable (e.g. `crop = 0`, `smoothing_factor = 0`, `frame_scale = 0` if callers ever want
them). If the strict positivity is a deliberate invariant, document it on the type/constructor;
otherwise allow `0.0` and only reject negatives/NaN/infinity.
Also consider implementing `TryFrom<f32>` (and keeping `try_new` delegating to it) so the type
exposes the idiomatic conversion trait instead of a bespoke constructor.



─── ukeela-cli/src/run.rs:51-51 ───
[bug · medium] The result of `tokio::signal::ctrl_c()` is discarded. It returns a `Result` (e.g.
`Err` when the signal handler cannot be registered); in that case the future resolves immediately
with `Err`, so the `select!` arm fires right away and the pipeline is shut down even though the user
never pressed Ctrl+C. Handle the error explicitly and only shut down on `Ok(())`.

-         _ctrl_c_result = tokio::signal::ctrl_c() => {
+         ctrl_c_result = tokio::signal::ctrl_c() => {
+             if let Err(error) = ctrl_c_result {
+                 tracing::error!(%error, "Failed to listen for Ctrl+C");
+             }


─── ukeela-common/src/frame.rs:95-95 ───
[bug · medium] Frame::new accepts width/height independently and never validates them against
data.dimensions(), so Frame.width/height can diverge from the pixel buffer. Consumers that trust the
declared dimensions misbehave: grayscale matrix construction uses buffer rows for height and
frame.width for width; overlay bounds checks use frame.width/height while writing through the
ImageBuffer (put_pixel panics out of bounds); resize derives destination dimensions from
frame.width/height while resizing frame.data. Derive dimensions from the buffer (or validate/assert
equality in Frame::new) and have consumers use the buffer's own dimensions so output geometry always
matches the pixel data.



─── ukeela-common/src/frame.rs:14-15 ───
[bug · medium] `Frame` derives `Default`, so `Frame::default()` has `id == 0`, while `FRAME_COUNTER`
also starts at 0 and hands `id == 0` to the very first frame created by `Frame::new`. Any code that
treats `frame.id` as a unique identifier (it is logged as `frame_id` in the processing chain and in
the gstreamer output) cannot distinguish a default-constructed/placeholder frame from the first real
frame. Start the counter at 1, or drop the `Default` derive and implement it with a distinct
sentinel id.



─── ukeela-common/src/frame.rs:100-102 ───
[bug · medium] Two problems with deriving the timestamp from the wall clock here:
1. `expect("duration is always positive")` panics inside a library constructor if the system clock
is ever before the Unix epoch (misconfigured/RTC-not-set devices); this is recoverable and should
not abort the process.
2. `UtcDateTime::now()` is not monotonic: an NTP/clock adjustment can make later frames carry
smaller timestamps, and the value is the *decode-time* wall clock, not the capture time. Downstream
it is converted directly into a GStreamer PTS
(`ClockTime::from_nseconds(frame.timestamp.as_nanos())`), so a wall-clock base (~1.7e18 ns) plus
non-monotonic jumps degrades pacing/sync.
Prefer taking the timestamp from the producer (buffer PTS) or a monotonic `Instant` base, and avoid
`expect` in the constructor.



─── ukeela-common/src/middleware.rs:13-13 ───
[maintainability · medium] The returned future is required to be `Sync`, but nothing in the codebase
needs that. Callers only `.await` the future inside a `tokio::spawn`ed task
(`ukeela-processing-chain/src/multi.rs:55`), which just requires `Send`. Requiring `Sync` on a
future is very restrictive: any middleware that holds a non-`Sync` value across an `.await` (e.g. a
`futures::lock::MutexGuard`, or a `Cell`/`UnsafeCell`-based guard) cannot implement this trait at
all, so it silently constrains every future middleware. It is also worth giving `process_async` a
default body that delegates to the sync `process`, so sync-only middleware does not have to
duplicate the same logic in both methods (the two paths can currently drift apart).

-     fn process_async(&self, frame: Frame) -> impl Future<Output = ProcessResult> + Send + Sync;
+     fn process_async(&self, frame: Frame) -> impl Future<Output = ProcessResult> + Send {
+         async move { self.process(frame) }
+     }


─── ukeela-middleware/ukeela-middleware-grayscale/src/lib.rs:29-29 ───
[performance · medium] This copies the whole frame buffer (`width * height * 4` bytes) on every
frame, even when the `Arc` is uniquely owned (the common case in a pipeline). Since `frame` is
consumed here, the data can be moved out instead of copied: take the `Arc` out of the frame (e.g.
`std::mem::take(&mut frame.data)`) and use `Arc::try_unwrap(...).unwrap_or_else(|arc|
(*arc).clone())`, which avoids the allocation/copy when the frame is not shared.



─── ukeela-middleware/ukeela-middleware-optical-flow-estimation/src/lib.rs:39-39 ───
[performance · medium] The RNG mutex is held for the entire estimation: 2000 RANSAC iterations plus
two refinement passes, each scanning all matches. Because the `Arc<Mutex<State>>` is shared by all
clones, this serialises every frame processed concurrently and turns a parallel pipeline into a
single-threaded bottleneck; it also makes the sampling sequence depend on thread scheduling, so
results are not reproducible anyway. Consider drawing the sample indices (or seeding a cheap
per-call/thread-local RNG) while briefly holding the lock and dropping the guard before the heavy
loop.



─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/lib.rs:42-43 ───
[performance · medium] `gray_image_from_matrix` unconditionally deep-copies the whole grayscale
buffer (`matrix.data.clone()`) on every call. This helper is called by both middlewares per frame
(feature detection + motion tracking), so a 1080p frame gets its ~2 MB gray buffer copied at least
twice per pipeline iteration, even though the source matrix is already shared behind `Arc`. Consider
converting once and caching the `GrayImage` (or letting callers pass ownership / reuse the converted
image) instead of re-cloning in the hot path. Also, because `GrayImage::from_raw` only checks `rows
* cols == data.len()`, a non-single-channel matrix falls through to the generic "dimensions do not
match its data" error, which is misleading; validating the element size/shape explicitly would give
a clearer diagnostic.



─── ukeela-cli/src/middleware.rs:89-90 ───
[bug · medium] The crop conversion reports the wrong flag name: a failure here is caused by `crop`,
but the returned error says "invalid frame scale". Since all three conversions use the same generic
wording, a user (or a caller passing crop values outside `Percent`'s `(0.0, 1.0]` range) would be
told to fix the wrong option. Make the message identify the correct flag (and ideally the offending
value) so the diagnosis is actionable.

-     let stabilization_crop =
-         Percent::try_new(f32::from(crop) / 100.0).ok_or(anyhow::anyhow!("invalid frame scale"))?;
+     let stabilization_crop = Percent::try_new(f32::from(crop) / 100.0)
+         .ok_or_else(|| anyhow::anyhow!("invalid frame crop: {crop}% (expected within 1..=100)"))?;


─── ukeela-cli/src/bin/main.rs:78-81 ───
[bug · medium] `from_env()` parses the user-controlled `RUST_LOG` environment variable and returns
`Err` when it contains a malformed directive (e.g. `RUST_LOG=garbage`). The `.expect()` therefore
panics in a production CLI path, and the message ("default level is set") is misleading because the
failure is unrelated to the default directive. Handle the error and fall back to the CLI-provided
level instead of panicking.

-     let filter = tracing_subscriber::EnvFilter::builder()
+     let filter = match tracing_subscriber::EnvFilter::builder()
          .with_default_directive(log_level.into())
          .from_env()
-         .expect("default level is set")
+     {
+         Ok(filter) => filter,
+         Err(err) => {
+             eprintln!("ignoring invalid RUST_LOG ({err}); using {log_level}");
+             tracing_subscriber::EnvFilter::builder()
+                 .with_default_directive(log_level.into())
+                 .parse_lossy("")
+         }
+     }


─── ukeela-cli/src/bin/main.rs:24-26 ───
[bug · medium] Unlike `processing_type`, `show_fps`, `log_level` and all `ProcessingFlags` (which
are `global = true`), `--json` and `--log-output-dir` are not global. Because every real argument
lives in a nested subcommand (`ukeela file <path> display`), these two flags can only be passed
before the subcommand and are rejected in the natural position (`ukeela file in.mp4 display
--json`). Make them `global = true` for consistency and usability.

      /// Format logs as JSON
-     #[arg(long, default_value_t = false)]
+     #[arg(long, global = true, default_value_t = false)]
      pub json: bool,


─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/src/lib.rs:1-4 ───
[maintainability · medium] None of these imports are referenced in this file; they only exist so
that `feature_detection.rs`/`motion_tracking.rs` can pull them in via `use super::*`. That creates a
hidden, fragile contract: removing or adding a dependency in a submodule requires silently editing
the crate root, and the root imports look unused at a glance (while a leftover import turns into an
`unused_imports` warning once no submodule references it). Prefer importing the needed items
directly in each submodule (`use image::Rgba;`, `use ukeela_common::{Frame, ...};`) or, if the
shared root import is intentional, mark it explicitly so the intent is visible:

```rust
pub(crate) use image::Rgba;
pub(crate) use std::sync::Arc;
pub(crate) use ukeela_common::{Frame, FrameData, Middleware, ProcessResult};
```



─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/src/feature_detection.rs:39-45 ───
[security · medium] `radius` is derived from `point.size` without any upper bound. A
non-finite/large `size` saturates the cast (`f32 as i32` → `i32::MAX`), after which `radius *
radius` overflows (panic in debug, wrap in release) and the `-radius..=radius` double loop
effectively hangs the worker (O(radius²) per point) — a cheap DoS if keypoint sizes come from
untrusted input. Likewise `px + dx` can overflow for saturated `pt.x`. Clamp the radius to a sane
maximum and/or validate the point coordinates.

-             let radius = if point.size > 0.0 {
-                 ((point.size * 0.5).max(1.0)).round() as i32
+             let radius = if point.size.is_finite() && point.size > 0.0 {
+                 ((point.size * 0.5).max(1.0)).round().clamp(1.0, 64.0) as i32
              } else {
                  2
              };
  
+             // `radius` is clamped above, so the square cannot overflow.
              let r_sq = radius * radius;


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/feature_detection/mod.rs:7-11 ───
[bug · medium] This single config is shared by two detectors with different capabilities, and
`distance_to_border` is silently ignored by one of them:
`FastFeatureDetectionMiddleware::process_frame` (fast.rs) only sorts/truncates the detected
keypoints and never filters points within `distance_to_border` of the frame edge, while
`OrbFeatureDetectionMiddleware` (orb.rs) does apply it via `set_edge_threshold`. Callers who set
`distance_to_border` expecting border filtering will get border-adjacent keypoints when using the
FAST detector. Consider giving each detector its own config (or at least documenting/enforcing that
FAST must also apply the border margin).



─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/feature_detection.rs:29-29 ───
[performance · medium] The two detection tuning values are hardcoded and none of them come from
`FeatureDetectionConfig` (which only exposes `num_features`/`distance_to_border`), so callers cannot
tune them even though the config exists for exactly that purpose. More importantly, passing `0` as
the minimum distance disables non-maximum suppression: on textured frames `good_features_to_track`
can return a very large, spatially clustered candidate set for every pixel above threshold, which is
then thrown away by `take(self.max_features)`. That is wasted CPU/allocation in a per-frame hot
path, and because `take` only keeps the first N returned while suppressing nothing, the retained
features may all cluster in one high-texture region, degrading subsequent tracking. Consider a
non-zero minimum distance and exposing both values (plus the confidence/quality level) on
`FeatureDetectionConfig`.



─── ukeela-pipeline/src/output.rs:14-19 ───
[bug · medium] `.ok().flatten()` collapses two semantically different outcomes into a single `None`:
the timeout elapsing (`Elapsed`) and the producer side being dropped / the channel closed. A caller
cannot tell whether it should keep polling (frame simply not ready yet) or shut down (pipeline
terminated), so this API invites premature termination on a slow frame or an endless retry loop on a
closed channel. In a real-time pipeline that distinction matters. Propagate the `Elapsed` error (or
return an explicit enum) so callers can decide, and document the semantics. `receive_frame()` is
also completely undocumented for a `pub` method of a library crate.

-     pub async fn recv_with_timeout(&mut self, timeout: std::time::Duration) -> Option<Frame> {
-         tokio::time::timeout(timeout, self.receive_frame())
-             .await
-             .ok()
-             .flatten()
+     /// Receive the next frame.
+     ///
+     /// Returns `None` once the producing side has been dropped (channel closed).
+     pub async fn receive_frame(&mut self) -> Option<Frame> {
+         self.rx.recv().await
+     }
+ 
+     /// Receive the next frame, failing with [`tokio::time::error::Elapsed`] if
+     /// no frame arrives within `timeout`.
+     ///
+     /// `Ok(None)` means the channel was closed; `Err(_)` means the timeout expired.
+     pub async fn recv_with_timeout(
+         &mut self,
+         timeout: std::time::Duration,
+     ) -> Result<Option<Frame>, tokio::time::error::Elapsed> {
+         tokio::time::timeout(timeout, self.receive_frame()).await
      }


─── ukeela-middleware/ukeela-middleware-optical-flow-lk/src/motion_tracking.rs:61-62 ───
[bug · medium] `track_fb` results are zipped with the input `previous_points` without verifying that
the two have the same length. If the tracker ever returns a different number of entries (skipping a
point that fell outside the pyramid, early-terminating, etc.), `zip` silently truncates and pairs
`previous` coordinates from frame N with `current` coordinates belonging to a different point. Those
corrupted `MotionMatch`es are written to `frame.metadata.motion_matches` and feed downstream
optical-flow estimation with no error surfaced. Add an explicit length check
(`anyhow::ensure!`/`debug_assert_eq!`) before zipping so a contract violation fails loudly instead
of producing wrong motion estimates.



─── ukeela-processing-chain/src/lib.rs:11-11 ───
[maintainability · medium] Returning `impl Future` from a trait method (RPITIT) makes
`ProcessingChain` non-dyn-compatible, so `Box<dyn ProcessingChain<T>>`, `&mut dyn
ProcessingChain<T>`, or storing chains of different concrete types is impossible without a wrapper.
If trait objects are intended anywhere (e.g. choosing between `SingleProcessingChain` and
`MultiProcessingChain` at runtime), this is a blocking design issue; the alternative is to return a
boxed future.

Also worth documenting: the returned future borrows `&mut self`, so callers cannot
`push`/`add_middleware` until `pop` is awaited or dropped.

-     fn pop(&mut self) -> impl std::future::Future<Output = Option<ProcessResult>> + Send;
+     fn pop(
+         &mut self,
+     ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ProcessResult>> + Send + '_>>;


─── ukeela-processing-chain/src/lib.rs:9-9 ───
[documentation · medium] `push` signals "not enqueued" by returning the frame in an `Option`, but
the returned frame is easy to ignore, in which case a frame is silently dropped (see
`SingleProcessingChain::push`, which replaces and discards the pending frame). This method is on a
hot path where frame loss is a data-loss bug, so the contract needs to be explicit and the value
hard to ignore. Note that `#[must_use]` on a trait method declaration is not reliably enforced, so
documenting it (or returning a typed enum such as `PushOutcome::Accepted/Dropped(Frame)`) is the
safer option.

+     /// Enqueues `frame` into the chain.
+     ///
+     /// Returns `Some(frame)` when the chain could not accept the new frame and
+     /// hands it back to the caller; the caller is responsible for handling (not
+     /// discarding) the returned frame.
      fn push(&mut self, frame: Frame) -> Option<Frame>;


─── ukeela-pipeline/src/input.rs:43-44 ───
[bug · medium] `FrameMetadata::order` is documented in `ukeela-common/src/frame.rs` as the
"Sequential output order assigned after pacing and processing", but it is populated here at ingress
with the same value as `original_order`. Since nothing downstream ever reassigns it, the frame
advertises an output order before any pacing/reordering happened, and diagnostics such as the
`changed_order` field logged in `ukeela-source-gstreamer/src/output.rs` can never observe a reorder
(`order == original_order` by construction). Leave `order` to the pacing stage and only stamp
`original_order` here.

          frame.metadata.original_order = Some(self.sequence);
-         frame.metadata.order = Some(self.sequence);


─── ukeela-pipeline/src/input.rs:26-26 ───
[maintainability · medium] send_frame is an async fn with no .await/yield point, so it never applies
backpressure and silently overwrites the previous unconsumed frame in the watch slot;
TrySendError::Full can never be produced by send_replace, so the Full(frame) arm is dead code and
SendError<Frame> only means closed. Its bool return is always Ok(true) and conveys no information.
Make the API synchronous or explicitly try_send-like with a meaningful outcome, and drop the
meaningless bool (or return whether an unconsumed frame was replaced).

-     pub async fn send_frame(&mut self, frame: Frame) -> Result<(), SendError<Frame>> {
+     pub fn send_frame(&mut self, frame: Frame) -> Result<(), SendError<Frame>> {


─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/motion_tracking.rs:59-61 ───
[bug · medium] There is no check that the previous frame and the current frame have the same
dimensions before running optical flow. Only the *candidate points* are filtered against the
previous image bounds, which is not sufficient: `calc_optical_flow_pyramid_lk` still receives two
images with different sizes, so either the library returns an error (failing the whole frame through
`?`) or it silently produces meaningless displacement values because pyramid levels/coordinates are
scaled differently. A resolution change (camera renegotiation, an upstream resize stage) is enough
to trigger this. The sibling implementation
`ukeela-middleware-optical-flow-lk/src/motion_tracking.rs` explicitly guards this case by
remembering the previous dimensions and skipping tracking when they differ; the same guard (or an
explicit state reset) should be used here.



─── ukeela-middleware/ukeela-middleware-optical-flow-pcv/src/motion_tracking.rs:11-15 ───
[maintainability · medium] The derived `Debug` implementation is reachable from the processing
chains, which themselves derive `Debug` (`SingleProcessingChain<T>`/`MultiProcessingChain<T>`), so
any `{:?}` of the chain formats this middleware's `State`, including the entire `previous_gray`
image buffer and the full point list. That can allocate megabytes of text and flood logs. The
equivalent `OpticalFlowLkMatchesMiddleware` deliberately implements `Debug` by hand with
`finish_non_exhaustive()` and only prints `min_matches`; this struct should do the same instead of
deriving `Debug`.

- #[derive(Debug, Clone)]
+ #[derive(Clone)]
  pub struct MotionMatchesMiddleware {
      min_matches: usize,
      state: Arc<Mutex<State>>,
+ }
+ 
+ impl std::fmt::Debug for MotionMatchesMiddleware {
+     fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
+         formatter
+             .debug_struct("MotionMatchesMiddleware")
+             .field("min_matches", &self.min_matches)
+             .finish_non_exhaustive()
+     }
  }


─── ukeela-processing-chain/src/multi.rs:47-47 ───
[bug · medium] This `unreachable!` encodes a data-dependent invariant but runs inside a detached
task: if any upstream stage (or a future change to `push`/stage forwarding) ever sends `Err`, the
worker panics, the task unwinds, `new_sender` is dropped and the downstream stage sees the channel
closed — which surfaces to the consumer as `pop() == None`, i.e. a silent pipeline stop. Prefer
handling the error defensively (log + continue, or forward it) instead of panicking on data.

-                         Some(Err(_)) => unreachable!("error variant should be dropped in previous"),
+                         Some(Err(e)) => {
+                             tracing::error!(error = ?e, "Middleware error received by worker");
+                             continue;
+                         }


─── ukeela-processing-chain/src/multi.rs:42-42 ───
[bug · medium] `tokio::spawn` here has two problems: (1) the `JoinHandle` is dropped, so the stage's
termination and panics are never observed — a middleware panic (including the `unreachable!` arms)
silently closes the output channel and makes `pop()` return `None`, which is indistinguishable from
a normal shutdown; (2) `add_middleware` is a synchronous trait method, so `tokio::spawn` panics with
"there is no reactor running" if it is ever called outside a Tokio runtime, and this API has no way
to report that. Consider keeping the handles (e.g. to abort/await on drop or to surface panics as
`Err`), or at least documenting the runtime requirement; wrapping the processing in
`AssertUnwindSafe` + `catch_unwind` would let failures be forwarded as `Err` instead of killing the
stage.



─── ukeela-pipeline/src/lib.rs:34-37 ───
[bug · medium] This public constructor panics on an ordinary invalid argument (zero interval). For a
library API, prefer returning a typed error (e.g. `Err(PipelineError::ZeroTelemetryInterval)`) or
silently clamping to a sane minimum; panicking forces every caller to pre-validate. The same applies
implicitly to being called outside a Tokio runtime (`interval_at`/`spawn` both panic), which is
worth documenting on the public signature.

-         assert!(
-             !telemetry_interval.is_zero(),
-             "telemetry interval must be greater than zero"
-         );
+         // Prefer: return Result<(Self, InputHandle, OutputHandle), PipelineError> and
+         // reject/clamp a zero interval instead of panicking.


─── ukeela-pipeline/src/lib.rs:58-58 ───
[bug · medium] The `JoinHandle` returned by `tokio::spawn` is discarded, so a panic inside the
processing loop is completely silent: `closed()` only waits on `broadcast::Sender::closed()`, i.e.
on all `Receiver`s being dropped, not on the task actually finishing. Keep the handle in
`FramePipeline` and await it in `closed()` (reporting `JoinError`), so task termination — including
panics — is observable. Note also that `closed()` returns as soon as every subscriber (including any
handle returned by `subscribe()`) is dropped, which is a different condition than "pipeline
finished".



─── ukeela-pipeline/src/lib.rs:99-100 ───
[bug · medium] `middleware_processing_times` is *replaced* (not accumulated) for every processed
frame, so when more than one frame is processed between two telemetry ticks, only the last frame's
per-middleware durations are ever logged — the earlier ones are silently discarded. Accumulate
(`entry(name).or_default() += duration`) and clear only in the telemetry branch, or move the
collection into the telemetry tick to aggregate over the whole interval. Also note that `mem::take`
strips `processing_time` from the frame before it is forwarded to the output, so downstream
consumers can no longer see those timings.



─── ukeela-source/ukeela-source-gstreamer/src/input.rs:98-101 ───
[bug · medium] Several exit paths leave the GStreamer pipeline running without calling
frame_pipeline.shutdown(): the early return when bus() is None leaves the pipeline in Playing
forever, and the EOS/Error branches break without shutting it down. Since run awaits
FramePipeline::closed(), these paths hang indefinitely (with the device still open) instead of
terminating. Transition to Null and notify the frame pipeline before returning/after the loop on
every exit path.



─── ukeela-middleware/ukeela-middleware-resize/src/lib.rs:40-41 ───
[bug · medium] `unwrap_or(u32::MAX)` masks a failed conversion by substituting an arbitrary
dimension. If it ever triggers, `FrameData::new(u32::MAX, dst_height)` would attempt a gigantic
allocation (likely aborting the process), and the buffer width would no longer match the
`frame.width = scaled_width` assigned below. Since these dimensions come straight from `usize` frame
fields, propagate the failure instead of inventing a value so the pipeline receives a normal
`anyhow` error.

-         let dst_width = u32::try_from(scaled_width).unwrap_or(u32::MAX);
-         let dst_height = u32::try_from(scaled_height).unwrap_or(u32::MAX);
+         let dst_width = u32::try_from(scaled_width).context("scaled width exceeds u32::MAX")?;
+         let dst_height = u32::try_from(scaled_height).context("scaled height exceeds u32::MAX")?;


─── ukeela-source/ukeela-source-gstreamer/src/output.rs:160-161 ───
[bug · medium] `Buffer::with_size(..).expect(..)`, `buffer.get_mut().unwrap()` and
`map_writable().unwrap()` all panic inside the spawned task. A panic here skips
`frame_pipeline.shutdown()` and the pipeline teardown, so the rest of the application keeps waiting
on a pipeline that is never shut down. Handle the failure by logging and breaking out of the loop so
the cleanup code below still runs.



─── ukeela-source/ukeela-source-gstreamer/src/output.rs:174-174 ───
[other · medium] `appsrc.push_buffer` is a blocking call: once the downstream queue
(`max-bytes`/`max-buffers`) is full it blocks the current thread until there is room. Calling it
directly inside a `tokio::spawn` task stalls the async worker and, because this loop also consumes
`shutdown_rx`, shutdown signals are not observed while it blocks. Consider running the push on a
blocking thread / dedicated output thread, or configure `appsrc` with `is-live`/`leaky-type` limits
so it cannot block indefinitely.



─── ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs:250-256 ───
[other · medium] `process_async` moves the whole `process_frame` (including the shared-state
mutation in `update_trajectory`) into a fire-and-forget `spawn_blocking` task that is reached
through `self.clone()`. If the returned future is dropped (task abort on shutdown, pipeline
teardown), the `JoinHandle` is dropped but the blocking task keeps running and still advances
`camera_trajectory`/`smoothed_trajectory` for a frame that is never emitted. Because trajectory
state is order-sensitive, this leaves the stabilizer desynchronized from the emitted stream.
Consider doing the cheap, order-sensitive `update_trajectory` synchronously on the caller side and
only offloading the CPU-heavy warp/crop, or at least document that the async path must not be
cancelled.



─── ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs:44-50 ───
[bug · medium] `update_trajectory` returns `Option`, where `None` means "the estimate was unusable"
(non-finite values, degenerate scale, or a failed smoothing step), but `Context::context` on an
`Option` turns that into a hard `anyhow::Error`. Consequently a single malformed/degenerate relative
transform makes the whole frame fail; in `ukeela-processing-chain/src/multi.rs` such an error is
logged and the frame is `continue`d (dropped), so a burst of bad estimates silently drops frames
from the output stream instead of passing them through un-stabilized. Prefer an explicit
pass-through (`unwrap_or_else(identity)` / a dedicated enum) and use `with_context` only for
genuinely fatal failures.

          let correction = match frame.metadata.similarity_transform.as_ref() {
-             Some(relative) => Some(
-                 self.update_trajectory(relative)
-                     .context("could not update stabilization trajectory")?,
-             ),
+             Some(relative) => Some(self.update_trajectory(relative).unwrap_or_else(identity)),
              None => None,
          };


─── ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs:110-112 ───
[bug · medium] `smoothing_factor` is passed straight through as the *current* weight of the
exponential smoother, which inverts the intuitive meaning and is unbounded in effect: with `Percent`
allowed up to `1.0`, `smoothing_factor = 1.0` yields `smoothed == actual` and therefore an identity
correction (stabilization completely disabled), while very small values freeze the smoothed
trajectory near the first frame, so `correction = smoothed * inverse(actual)` grows without limit as
the camera keeps moving. That ever-growing correction is then applied by `warp` and only a fixed
`crop_percent` margin is removed, so the opaque black `Border::Constant` fill is pulled into the
final cropped output. Please document/verify the intended direction of the factor (e.g. use `1.0 -
weight`) and clamp the correction magnitude (translation/rotation) to what the crop margin can hide.



─── ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs:85-87 ───
[bug · medium] The frame geometry is rewritten here (cropped, and previously warped), but
`frame.metadata` is left untouched: `metadata.grayscale` still caches the pre-crop image, and
`feature_points`/`motion_matches` are still expressed in the pre-warp/pre-crop coordinate space.
Downstream middlewares reuse these values — e.g. the FAST/ORB feature detection middlewares only
recompute grayscale when `metadata.grayscale.is_none()` (see
`ukeela-middleware-optical-flow-pcv/src/feature_detection/{fast,orb}.rs`) and overlay middlewares
draw `feature_points` onto `frame.data` — so they will operate on a stale buffer with mismatched
dimensions or draw at offsets equal to the crop margins. Invalidate the cached grayscale and
adjust/clear the coordinate-carrying metadata (or document the required pipeline ordering).



─── Cargo.toml:21-21 ───
[maintainability · medium] No MSRV (`rust-version`) is declared even though the workspace uses
`edition = "2024"` and `resolver = "3"`, both of which require a recent toolchain (edition 2024
needs Rust 1.85+). Declare `rust-version` once in `[workspace.package]` and inherit it in every
member via `rust-version.workspace = true`, so toolchains/CI get an explicit, enforced minimum
instead of an opaque compile failure.

+ rust-version = "1.85"
  edition = "2024"


─── Cargo.toml:41-41 ───
[performance · medium] The workspace-level `tokio` dependency enables `features = ["full"]`, which
is force-unified onto every member. Library crates (pipeline, middlewares, sources,
processing-chain) only need `rt`/`rt-multi-thread`, `sync`, `macros`, etc.; `full` also pulls in
net/fs/process/signal and bloats build time/binary size. Narrow the workspace entry to the minimal
common feature set and let the few crates that need more add features locally (features are
additive, so the inherited `full` cannot be narrowed per crate).

- tokio = { version = "1.35", features = ["full"] }
+ tokio = { version = "1.35", default-features = false, features = ["macros", "rt-multi-thread", "sync", "time"] }


─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/Cargo.toml:11-11 ───
[maintainability · medium] Several manifests declare direct dependencies that are never referenced
in the crate sources: `anyhow` in `ukeela-middleware-optical-flow-overlay`, `ukeela-pipeline`, and
`ukeela-processing-chain`; `image` in `ukeela-middleware-optical-flow-pcv`, `-resize`, and
`-estimation`; `purecv` in `-estimation`; `gstreamer-base` and `ukeela-processing-chain` in
`ukeela-source-gstreamer`. Remove the unused entries (or wire them in / move them to
dev-dependencies only if tests actually use them) to avoid unnecessary build time and a misleading
public dependency surface.



─── clippy.toml:4-4 ───
[maintainability · medium] `clippy.toml` contains configuration that is either redundant or not
wired to any enabled lint: `allow-unwrap-in-consts`/`allow-unwrap-in-tests` are explicitly set to
their default `false`, and `allow-expect-in-tests` only affects `clippy::expect_used`, which is not
enabled in `[workspace.lints.clippy]`. Remove the redundant keys (or enable the matching lint /
document intent) so the file only records real policy deviations.

+ # requires `expect_used = "warn"` in [workspace.lints.clippy]
  allow-expect-in-tests = true


─── ukeela-common/Cargo.toml:16-16 ───
[maintainability · medium] `purecv` is inherited from the workspace entry, which declares `features
= ["simd", "parallel"]`. Because this is a library crate, those heavy features are force-unified
onto every downstream consumer and cannot be turned off without patching this crate — the checklist
calls for small, non-opinionated defaults in libraries. Consider declaring `purecv` locally without
the `simd`/`parallel` features (or exposing them behind an additive feature such as
`simd`/`parallel`) so consumers such as the middleware crates can opt in.

- purecv.workspace = true
+ purecv = { workspace = true, default-features = false }
+ 
+ [features]
+ simd = ["purecv/simd"]
+ parallel = ["purecv/parallel"]


─── ukeela-middleware/ukeela-middleware-optical-flow-estimation/Cargo.toml:11-11 ───
[bug · medium] `src/lib.rs` uses `rand::rngs::SmallRng`, which has historically been gated behind
the `small_rng` cargo feature, while the workspace dependency `rand = "0.10"` (root `Cargo.toml`)
enables no features. Nothing else in the workspace enables `small_rng`, so this crate currently
builds only if the feature happens to be unified in from another crate; that is fragile
(adding/removing an unrelated dependency can silently break the build, and the feature always gets
unified into every target including `no_std`/wasm consumers). Declare the feature this crate relies
on explicitly.

- rand.workspace = true
+ rand = { workspace = true, features = ["small_rng"] }


─── ukeela-common/src/types/percent.rs:1-1 ───
[bug · high] `Default` is derived for this newtype, which yields `Percent(0.0)` — a value that
`Percent::try_new` explicitly rejects (`scale <= 0.0 => None`). So the only unchecked constructor
violates the invariant the checked constructor enforces, letting code (tests, `#[derive(Default)]`
on wrappers, `..Default::default()`, serde/clap defaults) silently create an invalid `Percent`.
Either implement `Default` with a valid value (e.g. `Self(1.0)` / document the chosen identity) or
drop the derive.

- #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
+ impl Default for Percent {
+     /// 100%: the neutral element for scaling factors.
+     fn default() -> Self {
+         Self(1.0)
+     }
+ }


─── ukeela-cli/src/run.rs:59-59 ───
[bug · high] `FramePipeline::closed()` internally awaits `broadcast::Sender::closed()`, which only
completes once **all** receivers are dropped. `close_signal` is still alive in this scope (it is
only dropped when `run` returns), so this call can never complete and will always hit the internal 5
s timeout and emit the misleading "Failed to wait for resources to release FORCING shutdown" warning
on every run. Drop the receiver before waiting.

+     drop(close_signal);
      pipeline.closed().await;


─── ukeela-common/src/frame.rs:11-11 ───
[bug · high] FrameData is hard-coded to Rgba<u8> while Frame::format may be BGR/BGRA, and the type
never enforces or documents that the buffer layout matches format. Consumers trust the invariant:
the grayscale middleware derives channels from frame.format.channels() and builds a matrix from
frame.data.to_vec(), so for BGR it requests 3 channels over a 4-channel buffer, causing
Matrix::from_vec to panic or produce a wrong-stride matrix. Make the buffer type depend on format
(raw Vec<u8> + format, or an enum of ImageBuffers) or reject a mismatched format in Frame::new and
document the invariant; middleware should derive the channel count from the actual buffer and
validate it.



─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/src/feature_detection.rs:24-31 ───
[bug · high] Arc::get_mut returns None not only when other Arc strong references exist but also when
any Weak handle exists, so the preceding strong_count > 1 check does not guarantee success and the
expect can panic on a normal frame. This is a production middleware path that already returns
ProcessResult and must not abort the pipeline. Replace the manual check-then-clone plus
Arc::get_mut().expect(...) with Arc::make_mut, which handles both shared strong references and weak
references correctly and removes the panic.

-         // Unique ownership of the Arc buffer (Clone-on-Write)
-         if Arc::strong_count(&frame.data) > 1 {
-             let inner = (*frame.data).clone();
-             frame.data = Arc::new(inner);
-         }
- 
-         let raw_data =
-             Arc::get_mut(&mut frame.data).expect("Frame data should be uniquely owned after clone");
+         // Clone-on-Write: detach the buffer if it is shared (strong or weak refs)
+         let raw_data = Arc::make_mut(&mut frame.data);


─── ukeela-middleware/ukeela-middleware-optical-flow-overlay/src/motion_tracking.rs:64-66 ───
[bug · high] `draw_line` only rejects non-finite values, but the loop bound is driven by the raw
coordinate magnitude, not by the image size. `motion_matches` coordinates come from feature tracking
and are not guaranteed to stay near the frame dimensions (e.g. a large finite value like `1e30`).
Consequences:

1. `round() as i32` saturates to `i32::MIN`/`i32::MAX`, so `(end_x - x).abs()` can overflow and
panic in debug builds (and `i32::MIN.abs()` panics in release too when the value is exactly
`i32::MIN`).
2. Even without overflow, the Bresenham loop iterates `max(|dx|, |dy|)` times, so a far off-screen
endpoint makes this loop run for billions of iterations and effectively hangs the pipeline thread
(the per-pixel bounds check only skips writes, it never stops the walk).

Clamp/reject endpoints against the image bounds before running the loop so the iteration count is
bounded by the frame size.

-     let (mut x, mut y) = (start.0.round() as i32, start.1.round() as i32);
-     let (end_x, end_y) = (end.0.round() as i32, end.1.round() as i32);
+     let (width, height) = (image.width(), image.height());
+     if width == 0 || height == 0 {
+         return;
+     }
+     let (max_x, max_y) = ((width - 1) as f32, (height - 1) as f32);
+ 
+     // Reject lines that are completely outside the frame; otherwise clamp the
+     // endpoints so the walk is bounded by the image size.
+     if (start.0 < 0.0 && end.0 < 0.0)
+         || (start.1 < 0.0 && end.1 < 0.0)
+         || (start.0 > max_x && end.0 > max_x)
+         || (start.1 > max_y && end.1 > max_y)
+     {
+         return;
+     }
+ 
+     let (mut x, mut y) = (
+         start.0.round().clamp(0.0, max_x) as i32,
+         start.1.round().clamp(0.0, max_y) as i32,
+     );
+     let (end_x, end_y) = (
+         end.0.round().clamp(0.0, max_x) as i32,
+         end.1.round().clamp(0.0, max_y) as i32,
+     );
      let dx = (end_x - x).abs();


─── ukeela-pipeline/src/input.rs:37-37 ───
[bug · high] Shutdown detection is not latched. In the input handle, try_recv().is_ok() only sees a
pending message; once the single broadcast notification is consumed (or overwritten due to
channel(1)/Lagged, or Closed), later calls treat the pipeline as still running. shutdown_resubscribe
likewise only observes values sent after the call and cannot query an already-observed shutdown.
Latch the shutdown state on the handle and treat every non-Empty try_recv result as shutdown, so
callers can reliably check current state rather than relying only on a future signal.

-         if self.shutdown_rx.try_recv().is_ok() || self.tx.is_closed() {
+         match self.shutdown_rx.try_recv() {
+             // No signal pending yet, keep ingesting.
+             Err(broadcast::error::TryRecvError::Empty) => {}
+             // `Ok(())` = shutdown broadcast, `Lagged` = the only buffered signal was
+             // overwritten, `Closed` = the pipeline is gone. All mean: stop ingesting.
+             _ => self.shutting_down = true,
+         }
+ 
+         if self.shutting_down || self.tx.is_closed() {


─── ukeela-processing-chain/src/single.rs:35-35 ───
[bug · high] ProcessingChain::pop is not cancellation-safe, and this is observable in production:
SingleProcessingChain::pop takes the pending frame out before awaiting middleware processing, while
FramePipeline polls pop() inside a tokio::select! alongside push/shutdown/telemetry. Whenever
another select branch wins, the future is dropped and the in-flight frame is silently lost - not
counted in dropped_frames, never emitted, never re-entered. Keep the frame in pending_frame until
processing succeeds (or re-queue from a Drop guard), drive pop() to completion outside the select,
and/or document the non-cancellation-safe contract so callers do not poll it in select!/timeouts.

+     /// # Cancel safety
+     ///
+     /// This future is **not** cancellation safe: a frame taken from the pending slot
+     /// while a middleware is being awaited is lost if the future is dropped.
+     /// Do not poll it in `tokio::select!`, timeouts, or any other combinator that
+     /// can drop it after it has started processing.
      async fn pop(&mut self) -> Option<ProcessResult> {


─── ukeela-processing-chain/src/single.rs:45-45 ───
[bug · high] The `Notify` plumbing can never actually wake the waiting branch. `pop` borrows `self`
mutably for the whole duration of the returned future, so `push(&mut self, ..)` cannot run
concurrently with a suspended `pop`; therefore `notified.await` can never be completed by a real
`notify_one`. The only case where a permit exists is when `push` ran *before* `pop` was called,
which is already handled by the `pending_frame.take()` check above (the permit is consumed by
`enable()` and the loop just spins once before blocking forever). In practice the waiting `pop` is
only ever ended by cancellation from the caller's `select!`. Either store the shared state behind
interior mutability / a channel so `push` and `pop` can live on different tasks (as
`MultiProcessingChain` does), or remove the `Notify` interaction here so the API does not imply a
wake-up path that cannot happen.



─── ukeela-processing-chain/src/multi.rs:67-69 ───
[bug · high] Middleware failures are swallowed: the frame (and the `anyhow::Error`) is only logged
and then dropped with `continue`, so this chain implementation never yields `ProcessResult::Err`.
Callers that rely on the trait contract are affected — e.g. `FramePipeline`
(ukeela-pipeline/src/lib.rs:106) has an explicit `Some(Err(error))` branch that increments
`dropped_frames` and logs the failure, but it is dead code for this chain, so failed frames vanish
without being accounted for. `SingleProcessingChain` propagates the error via `return
Some(Err(error))`. Forward the error on `new_sender` so `pop()` can observe it (and adjust the
`Some(Err(_))` arm below accordingly).

                                  "Middleware processing failed"
                              );
+                             // Propagate the failure downstream so `pop()` can report it.
+                             if matches!(new_sender.try_send(Err(e)), Err(TrySendError::Closed(_))) {
+                                 break;
+                             }
                              continue;


─── ukeela-pipeline/src/lib.rs:102-104 ───
[bug · high] `output_tx.send(frame).await` is awaited inside a `select!` branch body, so while the
output channel is full the task stops polling `shutdown_rx` and `ingress_rx` entirely. If the
consumer stalls (receiver alive but not drained), `shutdown()` can never be observed and the task
hangs until the consumer resumes; `closed()` then just warns after 5s. Consider `try_send` and
counting the frame as dropped on `Full`, or select the send against shutdown explicitly
(`tokio::select! { _ = shutdown_rx.recv() => ..., r = output_tx.send(frame) => ... }`).



─── ukeela-source/ukeela-source-gstreamer/src/input.rs:39-40 ───
[bug · high] The URI is built by string concatenation, which breaks for paths containing spaces,
`#`, `?`, `%`, non-ASCII characters (and Windows drive/backslash paths). Additionally, when
`canonicalize` fails (missing file, permission denied) the relative `path` is silently used with a
`file://` prefix, producing a malformed URI that will only fail later inside the pipeline. Use
glib's URI helper and propagate the failure.

-                 let absolute_path = std::fs::canonicalize(&path).unwrap_or(path);
-                 let uri = format!("file://{}", absolute_path.to_string_lossy());
+                 let absolute_path = std::fs::canonicalize(&path)
+                     .with_context(|| format!("Failed to resolve input file: {}", path.display()))?;
+                 let uri = gstreamer::glib::filename_to_uri(&absolute_path, None)
+                     .context("Failed to convert input file path to URI")?
+                     .to_string();


─── ukeela-source/ukeela-source-gstreamer/src/input.rs:43-43 ───
[bug · high] No caps filter forces RGBA here (unlike the Camera/Libcamera branches), and
`emit-signals` is meaningless when `set_callbacks` is used. Decoded video from `uridecodebin` is
normally I420/NV12; `process_sample` then fails at `VideoFormat::from_str` (only bgr/bgra/rgba are
supported) or at `ImageBuffer::from_raw` (which requires exactly `width*height*4` bytes for
`Rgba<u8>`), so a sample error shuts down the whole frame pipeline for any ordinary video file.
Normalize the format before the appsink.

-                     "uridecodebin uri=\"{uri}\" ! appsink name=sink sync=true emit-signals=true"
+                     "uridecodebin uri=\"{uri}\" ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink sync=true"


─── ukeela-source/ukeela-source-gstreamer/src/output.rs:45-45 ───
[security · high] The file path is interpolated directly into a `gst_parse_launch` string. A path
containing `"` or `\` (or a trailing backslash) breaks out of the quoted `location="..."` value,
which at best makes `parse::launch` fail and at worst injects additional pipeline elements.
`to_string_lossy()` also silently rewrites non-UTF-8 paths, so frames get written to a different
path than requested. Escape the value (or better, parse the pipeline without `filesink` and set the
`location` property on the element).

-                     path.to_string_lossy(),
+                     path.to_string_lossy()
+                         .replace('\\', "\\\\")
+                         .replace('"', "\\\""),


─── ukeela-source/ukeela-source-gstreamer/src/output.rs:42-42 ───
[bug · high] The file pipeline feeds `x264enc` straight from `appsrc`, but the caps set below are
limited to BGR/BGRA/RGBA while `x264enc` only accepts YUV formats (I420/NV12/...). Caps negotiation
will fail (`not-negotiated`), so `GstOutputConfig::File` cannot produce output. Insert a
`videoconvert` between the source/overlay and the encoder.

-                      x264enc speed-preset=ultrafast tune=zerolatency ! \
+                      videoconvert ! x264enc speed-preset=ultrafast tune=zerolatency ! \


─── ukeela-source/ukeela-source-gstreamer/src/output.rs:32-32 ───
[bug · high] `overlay_stage` is built whenever `show_fps == true`, but the `FakeSink` branch never
appends it, while the lookup further down unconditionally requires an element named `fps_overlay` in
that case. `GstOutputWriter::try_new(GstOutputConfig::FakeSink, .., true)` therefore always fails
with "FPS display requested, but output pipeline has no textoverlay element named fps_overlay".
Either include the overlay in the fakesink pipeline or reject the combination up front with a clear
error.

-             GstOutputConfig::FakeSink => ("appsrc name=src ! fakesink sync=false".to_owned(), None),
+             GstOutputConfig::FakeSink => (
+                 format!("appsrc name=src{overlay_stage} ! fakesink sync=false"),
+                 None,
+             ),


─── ukeela-source/ukeela-source-gstreamer/src/output.rs:210-210 ───
[bug · high] No EOS is sent to `appsrc` before tearing the pipeline down. For the
`mp4mux`/`filesink` configuration the moov atom is only written when EOS reaches the muxer, so an
abrupt `NULL` transition (and likewise the early `return` on the `set_state` failure path) leaves an
unplayable/truncated MP4. Send EOS (ideally wait for the bus EOS/ERROR message) before setting the
pipeline to `Null`. Note that the bus is not watched at all, so downstream errors are never
surfaced.

+             let _ = appsrc.end_of_stream();
              let _ = pipeline.set_state(gstreamer::State::Null);


─── .gitignore:7-7 ───
[security · high] Only the exact file `.env` is ignored. Environment files are commonly split per
environment/developer (`.env.local`, `.env.production`, `.env.development`, `.env.*.local`), and
those variants frequently hold secrets (DB passwords, API keys). They will currently be tracked and
can be committed by accident. Ignore the variants while explicitly keeping the template file.

  .env
+ .env.*
+ !.env.example


─── ukeela-cli/src/run.rs:27-28 ───
[bug · critical] The two processing types are wired to the opposite chain implementations:
`ProcessingType::Single` builds a `MultiProcessingChain` (which spawns a dedicated tokio task per
middleware) and `ProcessingType::Multi` builds a `SingleProcessingChain` (which runs middlewares
inline in one task). This inverts the user-facing `--processing-type` semantics: `single` actually
runs multi-threaded and `multi` runs single-threaded. Swap the chain constructors.

          ProcessingType::Single => {
-             let mut chain = MultiProcessingChain::new();
+             let mut chain = SingleProcessingChain::new();


─── ukeela-middleware/ukeela-middleware-optical-flow-estimation/src/lib.rs:85-89 ───
[bug · critical] min_matches (and CLI motion_min_matches) is accepted unvalidated even though a
similarity transform requires at least two correspondences. With min_matches == 0 an empty match set
reaches random_range(0..0) and panics; with min_matches == 1 the while second == first loop can
never terminate while holding the shared mutex, hanging all middleware clones and the pipeline.
Guard len < 2 in estimate_motion and enforce min_matches >= 2 at construction/CLI (e.g. NonZero or a
checked constructor) so invalid input cannot reach the sampling loop.

-         let first = rng.random_range(0..valid_matches.len());
-         let mut second = rng.random_range(0..valid_matches.len());
-         while second == first {
-             second = rng.random_range(0..valid_matches.len());
+         let len = valid_matches.len();
+         if len < 2 {
+             return None;
          }
+ 
+         let first = rng.random_range(0..len);
+         let second = (first + rng.random_range(1..len)) % len;


─── ukeela-source/ukeela-source-gstreamer/src/input.rs:53-54 ───
[bug · critical] User-supplied GStreamer pipeline descriptions can panic at these unwraps.
gst_parse_launch returns the single element itself when the description has one element, and
by_name("sink")/by_name("src") can resolve to non-Pipeline/non-AppSrc elements, so
downcast(...).unwrap() aborts the process from configuration. Both input try_new and output
construction return anyhow::Result; propagate these as contextual errors instead of panicking.

              .downcast::<gstreamer::Pipeline>()
-             .unwrap();
+             .map_err(|_| anyhow!("GStreamer description did not produce a Pipeline"))?;



──────── Project Summary ────────

Project-Level Review Summary                                                                                                                                                                              
                                                                                                                                                                                                          
Top Issues                                                                                                                                                                                                
                                                                                                                                                                                                          
1. Panics are the primary error-handling mechanism, and they cascade into permanent failure.                                                                                                              
ukeela-common/src/extensions/lock.rs uses lock().expect("poisoned lock"), so any panic while a lock is held permanently poisons every later acquire_* call in the middleware state paths. The same pattern
repeats in library/pipeline code: ukeela-common/src/frame.rs (expect("duration is always positive")), .../optical-flow-overlay/src/feature_detection.rs (Arc::get_mut expect guarded by a strong_count    
check that doesn't imply get_mut success), .../ukeela-source-gstreamer/src/input.rs (downcast(..).unwrap() on user-supplied pipeline descriptions), .../output.rs (Buffer::with_size, map_writable().     
unwrap()), ukeela-cli/src/bin/main.rs (from_env().expect(...) on RUST_LOG), and ukeela-pipeline/src/lib.rs (panics on telemetry interval 0). Because these run in detached tasks, a panic silently ends   
the pipeline while the rest of the app waits on it.                                                                                                                                                       
2. Shutdown/teardown is not reliable end-to-end.                                                                                                                                                          
ukeela-pipeline/src/input.rs doesn't latch shutdown (try_recv().is_ok() only sees a pending broadcast on a channel(1)), so shutdown_resubscribe-style logic can lose the signal. ukeela-cli/src/run.rs    
awaits FramePipeline::closed(), which waits on broadcast::Sender::closed() — impossible while close_signal is still alive in scope, so it always hits the timeout. ukeela-processing-chain/src/multi.rs   
and ukeela-pipeline/src/lib.rs discard the JoinHandle, so task termination is unobservable. ukeela-source-gstreamer/src/output.rs never sends EOS to appsrc, so mp4mux output is unplayable, and          
push_buffer (blocking) is called inside tokio::spawn, stalling the worker that also consumes shutdown_rx. Several early-return paths in input.rs leave the GStreamer pipeline in Playing.                 
3. ProcessingChain contract is violated by both implementations, and the CLI wires them backwards.                                                                                                        
ukeela-cli/src/run.rs builds a MultiProcessingChain for ProcessingType::Single and vice versa. ukeela-processing-chain/src/multi.rs swallows middleware errors (continue after logging), so it never      
yields ProcessResult::Err despite the trait contract; it also has a data-dependent unreachable! in a detached task and conflates Full/Closed via into_inner(). ukeela-processing-chain/src/single.rs pop  
is not cancellation-safe and is polled inside a select! in FramePipeline, dropping in-flight frames; its Notify plumbing can never fire because pop borrows &mut self.                                    
4. Frame invariants are neither enforced nor documented, and middlewares trust them.                                                                                                                      
ukeela-common/src/frame.rs: FrameData is hard-coded to Rgba<u8> while frame.format may be BGR/BGRA (grayscale middleware derives channels from format.channels()), Frame::new never validates width/height
against data.dimensions(), derive(Default) gives id == 0, which FRAME_COUNTER also assigns to the first frame. ukeela-pipeline/src/input.rs sets metadata.order == original_order although it is          
documented as post-pacing order. ukeela-middleware/ukeela-middleware-stabilization/src/lib.rs rewrites geometry (warp + crop) but leaves metadata.grayscale, feature_points and motion_matches in the pre-
warp coordinate space.                                                                                                                                                                                    
5. User-supplied numeric config reaches unchecked arithmetic in hot paths.                                                                                                                                
ukeela-middleware-optical-flow-estimation/src/lib.rs: min_matches == 0 reaches random_range(0..0) (panic) and min_matches == 1 can loop forever in the while second == first search. .../optical-flow-    
overlay/src/feature_detection.rs: radius derived from point.size with no bound → i32 saturation, radius * radius overflow and effectively a hang. .../optical-flow-pcv/src/feature_detection/mod.rs:      
distance_to_border: u32 consumed as as i32, wrapping to a negative threshold in orb.rs (and silently ignored by fast.rs). ukeela-cli/src/args.rs: fps/width/height of 0 interpolate into caps strings. ...
/ukeela-pipeline/src/lib.rs: zero telemetry interval panics.                                                                                                                                              
6. Concurrency and per-frame allocation hot spots serialise or copy whole frames.                                                                                                                         
.../optical-flow-estimation/src/lib.rs holds the RNG Mutex for the entire RANSAC (2000 iterations + 2 refinements), turning parallel pipelines serial. .../optical-flow-pcv/src/feature_detection/orb.rs  
clones the whole middleware/detector per process_async instead of using Arc like the sibling crates. .../optical-flow-lk/src/lib.rs gray_image_from_matrix deep-copies the gray buffer, called twice per  
frame. .../optical-flow-grayscale/src/lib.rs copies width*height*4 bytes even though the Arc is uniquely owned. .../stabilization/src/lib.rs warps the full frame then copies the crop, roughly double the
output size per frame, and its spawn_blocking is fire-and-forget so state is lost if the future is dropped.                                                                                               
7. GStreamer source/sink pipelines are mis-negotiated and built by string interpolation.                                                                                                                  
ukeela-source-gstreamer/src/output.rs: x264enc is fed BGR/BGRA/RGBA caps with no videoconvert, so GstOutputConfig::File cannot produce output; FakeSink + show_fps always fails the fps_overlay lookup.   
input.rs: uridecodebin output is not forced to RGBA (unlike Camera/Libcamera), so process_sample fails on I420/NV12. Both files interpolate user paths into gst_parse_launch strings (location="..."),    
which breaks on ", \, spaces, #, ?, %, non-ASCII, and is an injection vector; input.rs builds the URI by concatenation and silently falls back to the relative path when canonicalize fails.              
8. Workspace dependency/metadata hygiene is inconsistent.                                                                                                                                                 
Cargo.toml: tokio with features = ["full"] is unified onto every library crate; no rust-version despite edition = "2024"/resolver = "3"; commented-out thiserror; ukeela-source-cam missing from          
[workspace.dependencies]. ukeela-common/Cargo.toml: purecv inherits ["simd", "parallel"], force-unified on all consumers. .../optical-flow-estimation/Cargo.toml: uses SmallRng while the workspace rand  
enables no small_rng feature — likely a build break. Several manifests (optical-flow-overlay, -pcv, -resize, -estimation, ukeela-pipeline, ukeela-processing-chain) declare unused deps; ukeela-cli/Cargo.
toml hard-codes version instead of version.workspace = true.                                                                                                                                              
9. Error information is systematically lost or mislabelled.                                                                                                                                               
ukeela-pipeline/src/output.rs collapses Elapsed and channel-closed into None via .ok().flatten(). .../ukeela-middleware-grayscale/src/lib.rs stringifies purecv errors, discarding type and source chain. 
ukeela-cli/src/middleware.rs reports crop failures as "invalid frame scale". .../optical-flow-estimation/src/lib.rs has inconsistent clearing semantics for similarity_transform (early return preserves  
it, failure overwrites with None).                                                                                                                                                                        
10. Public API design carries avoidable footguns.                                                                                                                                                         
ukeela-common/src/types/percent.rs: Percent stores a 0..=1 fraction (every call site divides by 100) and its derived Default produces 0.0, a value try_new rejects. ukeela-common/src/middleware.rs       
requires the returned future to be Sync although only Send is needed by tokio::spawn. ukeela-processing-chain/src/lib.rs is non-dyn-compatible (RPITIT + new() without where Self: Sized) and re-declares 
supertraits already on Middleware; SingleProcessingChain derives Default, adding a spurious T: Default bound. Public traits/macros (AcquireRwLock, define_middleware_dispatch) are undocumented.          
                                                                                                                                                                                                          
Module Hotspots                                                                                                                                                                                           
                                                                                                                                                                                                          
• ukeela-source/ukeela-source-gstreamer/ (input.rs, output.rs, lib.rs) — highest severity density in the repo: caps negotiation bugs that make whole output modes non-functional, injection/escaping in   
parse_launch, missing EOS, blocking calls in async, panicking unwraps, shutdown leaks.                                                                                                                    
• ukeela-middleware/ukeela-middleware-optical-flow-* (estimation, lk, pcv, overlay) — ~15 comments spanning panics, unbounded config, serialised RNG, whole-frame clones, inconsistent state advancement  
(motion_tracking.rs), fragile use super::* re-export contract in overlay/src/lib.rs.                                                                                                                      
• ukeela-processing-chain/src/ (lib.rs, single.rs, multi.rs) — correctness bugs in both implementations: swallowed errors, cancellation-unsafe pop, dead Notify, unreachable! in a detached task, dropped 
JoinHandle.                                                                                                                                                                                               
• ukeela-pipeline/src/ (lib.rs, input.rs, output.rs) — shutdown latching, dropped handle, select! starvation, misleading telemetry (avg_fps, replaced middleware_processing_times).                       
• ukeela-common/src/ (frame.rs, extensions/lock.rs, middleware.rs, types/) — invariant and API-design issues that every downstream crate inherits.                                                        
• ukeela-cli/src/ (run.rs, args.rs, bin/main.rs, middleware.rs) — inverted chain wiring, dead closed() await, CLI ergonomics (--json/--log-output-dir not global), wrong error flags, undiscoverable      
options.                                                                                                                                                                                                  
• Workspace manifests (Cargo.toml, ukeela-common/Cargo.toml, ukeela-cli/Cargo.toml, ukeela-source/ukeela-source-cam/Cargo.toml, clippy.toml, .gitignore) — ~10 comments despite being configuration only. 
                                                                                                                                                                                                          
Cross-Cutting Concerns                                                                                                                                                                                    
                                                                                                                                                                                                          
• expect/unwrap/panic! in library and production paths. Appears in ukeela-common/src/extensions/lock.rs, ukeela-common/src/frame.rs, ukeela-source-gstreamer/src/{input,output}.rs, optical-flow-         
overlay/src/feature_detection.rs, optical-flow-pcv/src/feature_detection/fast.rs, ukeela-cli/src/bin/main.rs, ukeela-pipeline/src/lib.rs. Combined with dropped JoinHandles, panics become silent pipeline
death rather than surfaced errors (ukeela-processing-chain/src/multi.rs, ukeela-pipeline/src/lib.rs).                                                                                                     
• Detached-task fire-and-forget. multi.rs tokio::spawn, stabilization/src/lib.rs spawn_blocking, output.rs push loop: state mutation and shutdown signals depend on tasks nobody observes.                
• Numeric configuration accepted without range validation. min_matches, radius, distance_to_border, fps/width/height, telemetry interval, smoothing_factor — all documented as tunables, none validated at
the boundary.                                                                                                                                                                                             
• Whole-frame copies and blocking work on the per-frame path. grayscale/src/lib.rs, optical-flow-lk/src/lib.rs (gray_image_from_matrix), optical-flow-pcv/.../orb.rs (middleware clone),                  
stabilization/src/lib.rs (warp + to_image), optical-flow-estimation/src/lib.rs (RNG mutex).                                                                                                               
• Undocumented public surface. extensions/lock.rs traits, middleware.rs define_middleware_dispatch, optical-flow-lk FeatureDetectionConfig, optical-flow-overlay/src/lib.rs (no crate docs), processing-  
chain/src/lib.rs trait, and clap args in ukeela-cli/src/args.rs (num_features, distance_to_border, fast_threshold, motion_min_matches).                                                                   
• User-facing string quality: typos and misleading messages. "unknow" (frame.rs), "erro" (multi.rs), "don't contains" (fast.rs), "pipine" (input.rs), it's→its (args.rs), "invalid frame scale" for crop  
(cli/middleware.rs), plus derive_more/clap naming mismatch (catmullrom vs catmull-rom) in args.rs.                                                                                                        
• Dead or unreachable code left in place. ukeela-pipeline/src/input.rs TrySendError::Full arm (impossible with send_replace), single.rs Notify plumbing, multi.rs unreachable!, output.rs emit-signals    
misuse, commented-out thiserror in Cargo.toml, empty [dependencies] in ukeela-source-cam/Cargo.toml, unused imports in optical-flow-overlay/src/lib.rs that exist only for use super::*.                  
• Missing test coverage implied by the above. No comment cites a test guarding frame-format/dimension invariants, shutdown latching, or the chain error contract — the areas with the most severe findings
are also unguarded.                                                                                                                                                                                       
                                                                                                                                                                                                          
Quick Wins                                                                                                                                                                                                
                                                                                                                                                                                                          
• Fix typos and message wording: "unknow" → "unknown" (ukeela-common/src/frame.rs), "erro" → "error" and merge the TrySendError arms (processing-chain/src/multi.rs), "don't contains" (optical-flow-pcv/.
../fast.rs), "pipine" (ukeela-source-gstreamer/src/input.rs), it's → its (ukeela-cli/src/args.rs), crop error flag (ukeela-cli/src/middleware.rs).                                                        
• Replace is_none() + as_ref().expect(...) in optical-flow-pcv/.../fast.rs with a single ok_or_else(anyhow::Error::…).                                                                                    
• Hoist RANSAC_THRESHOLD.powi(2) to a constant in optical-flow-estimation/src/lib.rs; name the 7.0/-1.0 keypoint constants in optical-flow-lk/src/feature_detection.rs.                                   
• Swap sort + truncate for a partial selection (select_nth_unstable_by) in optical-flow-pcv/.../fast.rs.                                                                                                  
• Remove .gitignore unanchored build/.bin/.build patterns and add .env.* / .env.*.local.                                                                                                                  
• Delete the commented-out thiserror line and the empty [dependencies] table in ukeela-source-cam/Cargo.toml; register ukeela-source-cam in [workspace.dependencies]; move version and add rust-version to
[workspace.package] and inherit them.                                                                                                                                                                     
• Narrow tokio to per-crate feature sets instead of workspace full; enable small_rng on rand (or switch RNG) in optical-flow-estimation/Cargo.toml — this one may currently block the build.              
• Mark --json and --log-output-dir as global = true in ukeela-cli/src/bin/main.rs; replace unwrap_or_default() on the home dir with a fallback to an absolute temp/log dir.                               
• Add /// docs to the four undocumented clap args in ukeela-cli/src/args.rs and align derive_more rename_all with clap's ValueEnum naming.                                                                
• Remove Default from ukeela-common/src/types/percent.rs (or make it Percent::try_new-consistent) and document the 0..=1 fraction semantics; consider renaming to Fraction.                               
• Replace Arc::get_mut(...).expect(...) in optical-flow-overlay/src/feature_detection.rs with a mem::take/Arc::make_mut or a returned anyhow error.                                                       
• Drop the unused imports in optical-flow-overlay/src/lib.rs and pull them directly in the submodules instead of relying on use super::*. 