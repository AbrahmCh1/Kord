# Changelog

All notable changes to Kord are listed here. Versions follow [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-03

First release.

### Headphones

- Reads the battery of AirPods and Beats (each earbud and the case, charging state,
  lid and in-ear sensors) from their Bluetooth LE advertisements, no pairing tricks needed.
- Tells headphones connected to *this* computer apart from ones playing elsewhere.
- Connect and disconnect from Kord (Windows: through the Bluetooth audio driver, like the
  Settings app; Linux: BlueZ).
- Lists every pair of headphones paired with the computer, any brand.

### Status card

- Pops up when you open the case, like on a phone: floating 3D-rendered earbuds and case,
  batteries and a Connect button. Hides when the case closes.
- Also opens from the tray icon.

### Main window

- Device cards with 3D art for AirPods Pro, AirPods, AirPods Max, Beats, Galaxy Buds,
  Sony, JBL, Jabra and Logitech, and generic over-ear and in-ear art for the rest.
- A page per device: batteries, Connect/Disconnect and its options.
- Settings and About pages, light and dark themes, English and Spanish.
- Only one Kord runs at a time; starting it again brings the window up.

### Features

- Ear detection: pause when an earbud comes out (or both), resume when it goes back in.
- Auto connect when you put an earbud in, auto play, pause on disconnect, default volume,
  switch the audio output on connect and to the speakers while both earbuds are out.
- Windows audio: low-latency mode (SBC instead of AAC), keep the link awake to avoid
  cut-off sounds, hands-free profile on/off.
- Microphone: make the headphones the default mic, input volume, record-and-play-back test.
- Connection and low-battery notifications, start with the system.

### Tools

- `kord scan`, `kord device`, `kord headless` for troubleshooting.
- `tools/find_models.py` finds openly licensed 3D models of headphones by brand;
  `tools/render_model.py` renders them into the app's sprites.

[0.1.0]: https://github.com/AbrahmCh1/Kord/releases/tag/v0.1.0
