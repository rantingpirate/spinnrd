This isn't so much a traditional roadmap as just a list of things I'd like 
to implement, loosely arranged by file (where applicable) and 
priority/fun/coolness.

# Short-term

## `main.rs`
- [ ] FIXME: In function `parse_options`: Un-escape commas and semicolons.
- [ ] TODO: Add command line options for whether to quit on spinfile write 
	and open errors.
- [ ] TODO: Implement custom log format via command line option.
- [ ] TODO: Implement user and group ID and name for filename substitution.

## `src/accel/fsaccel.rs`
- [ ] TODO: Log before aborting due to bad scale.

## `spinnr.sh`
- [ ] FIXME: Rotate touchscreen to match display!
- [ ] TODO: Proper option parsing.
- [ ] TODO: Add logging.
- [ ] TODO: Autogenerate with `build.rs` to add default spinfile, etc.
- [ ] TODO: Additional (xrandr?) args (e.g. `--quiet`).
- [ ] FEEP: Interactive output choice.

## Overall
- [ ] Separate backend code into separate file: make it as simple as possible 
	to add a new backend (as simple as adding a line to a macro 
invocation?).
- [ ] MOAR DOCUMENTATION! (comments EVERYWHERE).

# Long-term
- [ ] Add iio backend
- [ ] Read options from config file

## Packaging
Because for this to achieve its full potential, it needs to be mostly (or 
at least muchly) plug-and-play.

- [ ] .deb package
- [ ] .rpm package
- [ ] pacman package
- [ ] portage package *(?)*

In addition to the basics (make actual packages with install scripts), the 
following would be useful to figure out how to do:
- [ ] Autodetect login manager and add appropriate script.

# Very long-term

## Facecam backend
The idea behind the 'facecam' backend is to use image recognition with 
a device's user-facing camera to keep the display oriented in the same 
direction as their face. The image-recognition aspect would probably make 
for a good research paper. The other problem is how to do this without 
blocking other applications from using the camera, or whether that's even 
possible, let alone feasible...
