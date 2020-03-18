# Version 0.7.0 (Unreleased)

- *BREAKING* Reworked initialization so that the window event loop is on the main thread. See examples for how to use the style.
- *BREAKING* Removed xinput2 bindings
- *BREAKING* Removed harfbuzz bindings & freetype-sys dep
- Replaced decorum with the more popular ordered_float
- Updated parking_lot to 0.10
- Resolve warnings on latest nightly.
- [WIP] Replaced harfbuzz/freetype text with Ilmentite.
- Fixed color format mismatches when using multisampling. (resolves debug_assert on debug builds)
- Rename BinStyle position_t field to position and the enum from PositionTy to BinPosition
- Atlas now has `batch_cache_id` method for looking up multiple cache_id coords at once.

# Version 0.6.1 (November 30th, 2019)

- Fix panic when window is minimized on windows
- Remove rand dependency

# Version 0.6.0 (November 5th, 2019)

- Update vulkano to 0.16

# Version 0.5.0 (October 6th, 2019)

- Update freetype-sys to 0.9
- Removed X11 window backend. This was never really functional and intentions with it have faded.
- Removed native input options and input source options. Going along with the X11 window backend removal these aren't really needed. Winit will be used for all input handling for the foreseeable future. The Window/Surface abstraction, BasaltWindow, will still remain in place to making switching away from winit or adding other backends possible.

# Version 0.4.5 (October 6th, 2019)

- Scrolling is now properly handled on wayland.

# Version 0.4.4 (October 4th, 2019)

- Fallback to window's inner size when current extent is unavailable. This seems to be a better solution for wayland. It is unknown if this case will happen on xorg and if this is the correct behavior.

# Version 0.4.3 (October 4th, 2019)

- Fix issue with wayland where current extent is not always available by defaulting to window's default size given in the options. Further testing needed if this is the ideal behavior.
- Fix errors/warnings in bin.rs with recent nightly releases.

# Version 0.4.2 (September 10th, 2019)

- Updated freetype-sys to 0.8. Shouldn't be breaking.
- No longer send interface events when the window has the cursor captured.
