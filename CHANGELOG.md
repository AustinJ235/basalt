# Unreleased

- **BREAKING** `ilmenite` has been replaced by `cosmic-text`
  - `build_in_font` feature is now removed.
    - The default system font will be used instead.
  - `atlas::ImageData::ImtImageView`  is removed.
  - `atlas::Image::from_imt` method removed.
  - `SubImageCacheID::Glyph` now uses `cosmic_text::CacheKey`.
  - Various `ilmenite` enums have been replaced with native basalt enums.
    - `ImtTextWrap` -> `TextWrap`
    - `ImtVertAlign` -> `TextVertAlign`
    - `ImtHoriAlign` -> `TextHoriAlign`
    - `ImtWeight` -> `FontWeight`
  - Additional enums for font attributes have been added.
    - These include `FontStretch` and `FontStyle`.
    - `BinStyle` uses these as `font_stretch` and `font_style`.
  - `Interface::default_font` method now returns `DefaultFont` struct.
  - `Interface::set_default_font` method now takes `DefaultFont` struct.
  - `Interface::add_font` method removed.
  - `BstOptions` has had several changes.
    - `imt_gpu_accelerated` method removed.
    - `imt_fill_quality` method removed.
    - `imt_sample_quality` method removed.
    - `add_binary_font` method added.
      - This replaces the `Interface::add_font` method.
- **BREAKING** `Atlas` images now have metadata in the form of `Vec<u8>`
  - `load_image` methods now take additional metdata parameter.
  - `AtlasCoords` now has `metadata` method to retrieve metadata associated to the image.

# Version 0.20.0 (April 29th, 2023)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.33`.
- **BREAKING** Update dependency `ilmenite` to `0.14`.
- **BREAKING** Removed `enable_validation` from `BstOptions`.
- Update dependency `winit` to `0.28`.

# Version 0.19.1 (December 22nd, 2022)

- Fixed non-uniform access in interface shader.
- Fixed bug with slider not releasing when mouse left button is released.
- Fixed bug with slider calling on change methods when there wasn't a change.

# Version 0.19.0 (October 31st, 2022)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.32`.
- **BREAKING** Update dependency `ilmenite` to `0.13`.
- Custom Font Support
  - **BREAKING** `BinStyle` now has `font_family` & `font_weight` fields.
  - Added default feature `built_in_font` that allows disabling loading/including of built-in font.
  - `Interface` now has `default_font`, `set_default_font` & `add_font` methods.
- Changes to `BinStyle` updates/validation.
  - **BREAKING** `Bin::style_update` now returns `BinStyleValidation` which must be used.
    - Introduced non-default feature `style_validation_debug_on_drop` which will use the `debug` method of `BinStyleValidation` upon dropping. This also removes the `#[must_use]` attribute of `BinStyleValidation`.
  - Supress warnings about body being too small when using text.
- **BREAKING** Removed `seperate_` image methods on `Bin`.
  - Using seperate images not provided by the atlas should use the `back_image_raw` style of `BinStyle`.
  - **BREAKING** Removed `back_srgb_yuv` field from `BinStyle`.
    - This wasn't proper `yuv` supported as probably not used. For those that may be using this either use the `Atlas` for yuv conversion or convert the image to RGB and use the `back_image_raw` field.
- Added method `window_ref` to `Basalt`.

# Version 0.18.0 (September 18th, 2022)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.30`.
- **BREAKING** Update dependency `ilmenite` to `0.12`.
- **BREAKING** `Input` and `HookManager` (a.k.a. `BinHook`) have been rewritten/merged.
  - As this is a rewrite, the design has changed greatly. Please refer to docs.
  - Convenience methods on `Input` are now accessed on windows instead.
    - For example a press can be added via `basalt.window().on_press(...)`.
  - Convenience methods on `Bin` have had their named changed and some additional ones added.
    - For example `bin.on_mouse_press(...)` is now `bin.on_press(...)`.
  - More specific/customized hooks are created via builders accessed by `Input.hook()`.
  - Hook method signatures have changed away from an enum style and now have specific signatures for each type of hook.
  - Most data available for hooks is still available, but is accessed differently.
    - `on_hold` is an exception where querying the cursor position, or any other window state, is no longer available.
  - The concept of weights are now implemented.
    - This allows hooks to be processed in certain order and block the calling of other hooks of the same class. Refer to the docs for more specfic details on how this system works.
- **BREAKING** `BinStyle.pass_events` is now removed.
  - This has been removed in favor of the weight system.
  - This was introduced originally as sort of hack for having scrollable content within other scrollable content.
    - Specifically `InputScrollBuilder.upper_blocks(...)` replaces this hack.
- **BREAKING** `BinID` is now a concrete type instead of an alias.
- **BREAKING** `Interface.get_bin_id_atop(...)` and `Interface.get_bin_atop(...)` now additionally take a `BstWindowID`.
- **BREAKING** `ImageEffect` now has `GlyphWithColor` varient.
- **BREAKING** Removed `Basalt::physical_device_index()` as `vulkano` no longer has indexes for physical devices.
- Interface now has the methods `get_bins_atop(...)` and `get_bin_ids_atop` which function like their singular varients, but return a sorted `Vec` instead where the top most is first.
- New `Interval` system
  - This system introduces the ability to run a hook on a specfic interval down to 1 ms of precision.
    - The minimum resolution is platform/scheduler specific, but generally Windows is about 1.4ms and linux at 1 ms.
  - The rewritten `Input` system utilizes this functionality with `on_hold` and `on_scroll`, *where smooth scrolling is enabled*, to provide more frequent and consistent intervals.
- Changed font used to a more common, less quirky font Roboto.
- Improved subpixel sampling for glyphs in interface shaders.
- Added method `Basalt::physical_device_ref()`.

# Version 0.17.0 (August 19th, 2022)

- `Atlas` now internally uses the `guillotiere` crate for allocation.
- `atlas::Coords` has been reworked.
  - **BREAKING** Renamed to `AtlasCoords` to reduce name conflicts.
  - **BREAKING** All fields are now private.
  - **BEHAVIOR** `AtlasCoords` is now required to be kept alive for the coordinates to be valid.
    - Upon dropping `AtlasCoords` deletion may occur depending on behavior defined by `AtlasCacheCtrl` upon adding an image to the `Atlas`.
  - Added `external` method to allow construction for use in `BinStyle.back_image_raw_coords` and other use cases.
  - Added `is_external` method to check if `AtlasCoords` was created via `external`.
  - Added `none` method to create none/invalid `AtlasCoords` for use as a placeholder.
  - Added `is_none` to check if `AtlasCoords` was created via `none`.
  - Added `image_id` method to get the `AtlasImageID`.
  - Added `tlwh` for getting the top-left-width-height, size/position.
  - Internal positional and dimensional fields are represented as `f32` instead of `u32` to reduce coercion.
- `Atlas` image removal support.
  - Added enum `AtlasCacheCtrl` to define behavior around removal.
  - **BREAKING** `Atlas` methods `load_image`, `load_image_from_bytes`, `load_image_from_path`, and `load_image_from_url` now additionally takes `AtlasCacheCtrl` as an argument.
  - **BREAKING** Added `BinStyle` field `back_image_cache` to specify `AtlasCacheCtrl` used for various back image sources.
- **BREAKING** All methods taking signature of `Arc<Fn(_) -> _ + Send + Sync>` now take `FnMut(_) -> _ + Send + 'static`.
  - In most cases wrapping a closure in an `Arc` was arbitrary.
    - For users wanting to reuse functions:
        - Option A: Define function if there isn't a need to send variables to within the closure.
        - Option B: Call the `Arc<Fn(_) -> _ + ...>` from a closure, `|args| arcd_fn(args)`.
  - This change removes the need for `Sync` in types.
  - `'static` maybe appear as an additional requirment, but was previously implied.
  - `FnMut` is now used instead of `Fn` allowing state to be kept without the need of synchronization primatives.
- **BREAKING** `InputHookFn` type alias has been removed.
- **BREAKING** `InputHookRes` has been renamed to `InputHookCtrl`.
  - **BREAKING** `Error` & `Warning` varients are now removed.
    - Users should instead print the message themselves.
  - **BREAKING** `Success` varient has been renamed to `Retain`.
  - `InputHookCtrl` now implements `Default` and will default to `Retain`.
- **BREAKING** `BinHookFn` type alias has been removed.
- **BREAKING** `Bin::add_hook_raw` has been renamed to `Bin::add_hook`.
- **BREAKING** `Bin::on_update` & `Bin::on_update_once` now take `FnMut(bin: &Arc<Bin>, post_update: &PostUpdate)`.
- **BREAKING** `CheckBox::on_change` no longer spawns a thread to call method.
- **BREAKING** `Slider::on_change` no longer spawns a thread to call method.
- **BREAKING** `Atlas::new()` now takes `Arc<Queue>`, `VkFormat`, `max_alloc_size: u32` instead of `Arc<Basalt>`.
- **BREAKING** `basalt::Options` has been renamed to `BstOptions`.
- **BREAKING** `basalt::Limits` and the associated method `Basalt::limits()` has been removed.
  - Use `Basalt::physical_device().properties()` instead.
- **BREAKING** `InputHookData::Character { .. }` is now `InputHookData::Character(char)` and `BinHookData::Character { .. }` is now `BinHookData::Character(char)`.
  - No longer mapped from scan code. This allows non-US languages to work.
  - Use `'\r'` to detect new lines and `'\u{8}'` for backspaces.
- **BREAKING** Removed `Qwerty::into_char()`.
  - This was not proper to begin within. If you need to receive characters, use either `InputHook::Character` or `BinHook::Character`.
- **BREAKING** Removed methods `resize`, `enable_fullscreen`, `disable_fullscreen`, and `toggle_fullscreen` from `Basalt`.
  - Used methods on `BasaltWindow` returned by `Basalt::window()` instead.
- **BREAKING** `BasaltWindow::enable_fullscreen()` now takes `FullScreenBehavior` as an argument and returns `Result<(), FullScreenError>`.
- **BREAKING** `SubImageCacheID::Glyph` is now a struct not a tuple.
- `AtlasImage` now has the `load_from_bytes`, `load_from_path`, `load_from_url` methods that are used by the corresponding `Atlas` methods.
- `BstImageView` now has the `set_drop_fn` for setting a method to be called when all temporary views are dropped.
- `BinPosition`, `BstEvent`, & `BstWinEv` types that already derived `PartialEq` now also derive `Eq`.
- Fixed bug when input hooks return'd `InputHookRes::Remove` (now `InputHookCtrl::Remove`) it didn't actually do anything.
- `Bin` now has `on_children_added` & `on_children_removed` methods.
- Removed `unsafe` code from `Basalt` initialization which caused undefined behavior.
- Added `BstOptions::bin_parallel_threads` to specify amount of parallel threads used for `Bin` updates.
- Update dependancy `winit` to `0.27.2`.
- Additional `BasaltWindow` methods where added:
  - `is_fullscreen()`: check if the window is in fullscreen.
  - `monitors()`: returns a list of monitors.
    - Introduces `Monitor` and `MonitorMode` used in `FullScreenBehavior`.
  - `primary_monitor()` returns the primary monitor if determinable.
  - `current_monitor()` returns the current monitor if determinable.

# Version 0.16.1 (July 25th, 2022)

- Fixed `VUID-vkCmdDraw-None-02703` & `VUID-vkCmdDraw-None-02699` validation errors.

# Version 0.16.0 (July 20th, 2022)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.30`.
- **BREAKING** Update dependency `ilmenite` to `0.11`.
- **BREAKING** `BstFormatsInUse` now has `swapchain` & `swapchain_colorspace` fields used primary for app loop applications. This can double as a recommended swapchain format for other applications.
- **BREAKING** `misc` methods `partial_ord_min`, `partial_ord_min3`, `partial_ord_max`, & `partial_ord_max3` have been removed.
- **BREAKING** `PostUpdate` fields `pre_bound_min_y` & `pre_bound_max_y` have been replaced with `unbound_mm_y`.
- **BREAKING** `Bin::calc_overflow` has been renamed to `calc_vert_overflow`.
- **BEHAVIOR** `overflow_x` & `scroll_x` are now implemented on `BinStyle`. Previously overflowing horizontal content will now be removed.
  - Usage `overflow_x: Some(true)` to revert this behavior where needed.
- Added `conservative_draw` method to `Options`. This is currently experimental and will attempt to limit interface redraws for app loop applications.
- Fixed bug where if MSAA was in use with `ItfDrawTarget::Image` an error would occur.
- Fixed bug where overflow on a `Bin` was not calculated correctly with `calc_vert_overflow` method.
- `PostUpdate` added field `unbound_mm_x` for min/max of horizontal content before bound (overflow removal).
- `Bin` now has `calc_hori_overflow` method.
- Fixed bug where alternative `Atlas` formats weren't actually supported.
- Fixed bug where atlas formats were not checked for `transfer_src` support.
- Fixed bug where `OnOffButton` would never drop.
- `atlas::Image` now has `to_8b_srgba` & `to_8b_lrgba` methods.

# Version 0.15.0 (March 22nd, 2022)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.29`.
- **BREAKING** `Basalt::swap_caps()` has been replaced by `Basalt::surface_capabilities`, `Basalt::surface_formats`, & `Basalt::surface_present_modes`.
- **BREAKING** `Basalt::current_extent()` now takes `FullScreenExclusive` as an argument.
- **BREAKING** Renamed `Qwery` to `Qwerty`...
- **BREAKING** `BinColor::as_tuple()` has been removed and replaced by `BinColor::as_array()`.
- **BREAKING** `atlas::Coords` methods `top_left`, `top_right`, `bottom_left`, & `bottom_right` now return `[f32; 2]` instead of `(f32, f32)`.

- **BEHAVIOR** `Camera::mouse_inside()` will now return false for `Bin`'s that have `pass_events` set to `Some(true)`.
- Fixed various instances of buffers being created with zero size.
- Fixed wrong image values for Atlas's empty image.
- Refactored queue creation to have weights more inline with vulkan spec.

# Version 0.14.0 (January 5th, 2022)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.28`.

# Version 0.13.0 (December 7th, 2021)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.27.1`.
- **BREAKING** Update dependency `ilmenite` to `0.8.0`.
- **BREAKING** `BinStyle` now has `text_secret` field. If this property is set all text that appears will be replaced with `*`'s.
- `misc::http::get_bytes` will now follow redirects. This makes it possible when using `back_image_url` of `BinStyle` to use links that redirect.
- `Atlas` now will wait to return a response until image upload has been completed.
    - This solves issues like when entering text and the input not immeditely appearing.
- `Bin` now has `force_recursive_update` method which can be used to force an update on a bin itself and all of its children recursively.
    - There are still certain cases that may not properly trigger `Bin` updates. This method can be use to manually trigger an update.
- `Bin::drop()` will now awake the `Composer` resulting in dropped `Bin`'s being visually removed faster.
- Update dependency `winit` to `0.26.0`.

# Version 0.12.0 (October 3rd, 2021)

- **BREAKING** Update dependency `ilmenite` to `0.7.0`.
- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.26`.
- **BREAKING** Atlas has been reworked
    - **BREAKING** Renamed method `default_sampler` to `linear_sampler`.
    - Now uses 16 bit Linear formats for images instead of 8 bit SRGB.
    - Added method `nearest_sampler` that provides a nearest filter sampler.
    - Methods `load_image_from_bytes`, `load_image_from_path`, & `load_image_from_url` now support loading of 16 bit images fully instead of converting them to 8 bit.
    - Secondary graphics queue will be used instead of compute queue when available and will fallback to the primary graphics queue. This is needed as blit operations need to be done on a queue with support for blit operations.
    - `ImageData`
        - **BREAKING** No longer nonexhaustive.
        - **BREAKING** Added Varients
            - `D16`: 16 bit images
            - `Imt` for `ImtImageView`.
            - `Bst` for `BstImageView`.
    - `ImageType`
        - **BREAKING** No longer has `Glyph` varient. Use `SMono`.
        - **BREAKING** Added `Raw` varient that is used for `Image`'s constructed from `from_bst` & `from_imt`.
    - `Image`
        - Now implements `Debug` and `Clone`.
        - New constructors
            - `from_bst` for construction from `BstImageView`.
            - `from_imt` for construction from `ImtImageView`.
        - **BREAKING** `to_srgba` has been renamed to `to_16b_srgba` which converts `Image` to one constructed of `ImageType::SRGBA` & `ImageData::D16`. This does not effect `Image`'s of `ImageType::Raw`.
        - **BREAKING** `to_lrgba` has been renamed to `to_16b_lrgba` which converts `Image` to one constructed of `ImageType::LRGBA` & `ImageData::D16`. This does not effect `Image`'s of `ImageType::Raw`.
- **BREAKING** Interface rendering has been reworked.
    - **BREAKING** Moved `interface::interace` into `interface`.
    - **BREAKING** `ItfRenderer` is no longer public. Its public facing functionality is now moved to `Interface` via `draw`.
        - Draw method now takes a command buffer builder and a `ItfDrawTarget`.
            - `ItfDrawTarget` replaces the previous `win_size`, `resize`, `swap_imgs`, `render_to_swapchain` & `image_num`.
            - This limits the amount of fields only to what is required.
                - i.e. `ItfDrawTarget::Image` only needs an extent.
            - It is no longer required to pass resize when the swapchain is recreated or resized. Given the context the renderer will automatically recreate the target when needed.
            - `ItfDrawTarget` also has `SwapchainWithSource` which is recommended when doing additional graphics on top of basalt. This will render the source in the background and have proper blending with component aware alpha. This replaces the need to have a seperate pipeline where the end user would have to have a seperate pipeline to combine their image with Basalt's.
    - New rendering is done on a layer basis with component aware alpha blending which allows for more correct alpha blending and text blending along with more advance rendering features to come.
- **BREAKING** `BstImageView` temporary views have been reworked.
    - **BREAKING** `create_tmp` no longer returns an `AtomicBool`.
    - Added method `temporary_views` to fetch the amount of temporary views.
    - Added method `mark_stale` to mark the image view stale.
        - This is used on the parent view to hint to owners of temporary views that the view that is provided is stale and should be replaced.
    - Added method `is_stale` to check if the view is stale.
        - Check if the owner view has marked this view stale. If true the view should be replaced or dropped as soon as possible.
- **BREAKING** `Options` no longer has `interface_limit_draw`. Limiting the draw had weird quirks and is removed for the time being.
- **BREAKING** `input::Event::WindowScale` now expects scale to be provided.
- **BREAKING** `BstMSAALevel` has been moved into the `interface` mod.
- **BREAKING** `BstEvent::BstItfEv` & `BstItfEv` are no longer.
- **BREAKING** Changed how scale & msaa are accessed and handled.
    - **BREAKING**`Basalt::current_scale()` has been moved to `Interface::current_scale()`.
    - **BREAKING**`Basalt::set_scale()` has been moved to `Inteface::set_scale()`.
    - **BREAKING**`Basalt::add_scale()` has been removed.
    - **BREAKING**`Interface::msaa()` method has been renamed to `current_msaa`.
    - `Interface` now has the following methods added:
        - `current_effective_scale`: current scale taking into account dpi window scaling.
        - `set_effective_scale`: set the scale taking into account dpi window scaling.
- `Options` now has `imt_gpu_accelerated` to select whether ilmenite will use gpu accerated font rasterization. `imt_fill_quality` to select the fill quality for ilmenite, and `imt_sample_quality` to select the sample quality for ilmenite.
- Fixed bug where glyph alignment was incorrect when scale was not 100%.
- Bins will now load images into the atlas directly from ilmenite.
- Interface shader will now use nearest filter sampler when sampling glyph images. This resolves issues with subpixel hinting being incorrect.

# Version 0.11.2 (July 20th, 2021)

- **POTENTIALLY BREAKING** Basalt no longer uses all supported features, instead it only uses features it needs to function. For users that require additional features see `Options::with_features()`.
- `Options` now has `with_features()` method to specifiy additional features.
- Added method `basalt_required_vk_features()` to provide required features in order for Basalt to function.
- Pinned dependency `ilmenite` to `0.4.1` which implements and defaults to cpu rasterization. This solves issues with ilmenite compute shaders being incompatible with nvidia cards.

# Version 0.11.1 (July 8th, 2021)
- Remove use of `drain_filter` feature to allow compilation on stable.
- Upgrade crate to use edition 2018

# Version 0.11.0 (July 4th, 2021)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.24.0`
- **BREAKING** Update dependency `ilmenite` to `0.4.0`
- **BREAKING** `Basalt::should_recreate_swapchain()` has been replaced by `Basalt::poll_events()`. This allows some simplification in the backend along with allowing the user in on more of what is going on. `BstEvent::requires_swapchain_recreate()` and `BstEvent::requires_interface_redraw()` have been provided as helpers. `BASALT::poll_events().into_iter().any(|ev| ev.requires_swapchain_recreate())` would directly replace `Basalt::should_recreate_swapchain()`.
- Added `Basalt::current_extent()` as a helper to get the size of what the swapchain should be.
- Added `force_unix_backend_x11()` to `Options` which allows the user to perfer X11 over wayland. Mainly intended for users of wayland desktops that wish to use `Basalt::capture_cursor()` and receive
`MouseMotion` events.
- Fixed issues with colorspaces.

# Version 0.10.0 (May 29th, 2021)

- **BREAKING** Update dependency `vulkano` & `vulkano-shaders` to `0.23.0`
- **BREAKING** Update dependency `ilmenite` to `0.3.0`
- **BREAKING** `TmpImageViewAccess` has been removed and functionality replaced by `BstImageView`.
- **BREAKING** `Interface::set_msaa()` no longer returns a `Result` and now takes `BstMSAALevel`
instead of an integer.
- Fixed bug where when scale was set in `Options` it was not propagated to other default value for scale for interface related constructs.
- Scale is now able to be set in the command args using `--scale=2.0`. This will override any scale set by `Options`.
- `Options` now has `msaa()` function to set initial MSAA level.
- Update dependency `ordered-float` from `2.0` to `2.5`.
- Update dependency `arc-swap` from `1.2` to `1.3`.

# Version 0.9.0 (January 31st, 2021)

- **BREAKING** Update dependency `vulkano` to `0.20.0`
- **BREAKING** Update dependency `winit` to `0.24.0`
- **BREAKING** Update dependency `ilmenite` to `0.2.0`
- Update dependencies `image`, `parking_lot`, `crossbeam`, `num_cpus`, `curl`, `ordered-float`, and `arc-swap`.
- Added implementation for `BinPosition::Floating`. Bins with the positioning type must have width and height set only. `pos_from_[tblr]`, `pos_from_[tblr]_pct`, and `pos_from_[tblr]_offset` will result in a warning and the `Bin` rendering incorrectly. Additionally `margin_[tblr]` is now used for floating bins for spacing. `pad[tlbr]` is used on the parent to set the usable area with the parent bin. Note: Adding additional children to a parent with floating children that are not floating will result in weird behavior. Other positioning types are not takin into consideration when calculating the position of the floating children. If this is the intendted use case use a z-index override is suggested to get the intended z-index.
-  Added two additional attributes to the `BinStyle` struct, `width_offset` and `height_offset`. These are to be used in conjucntion with `width_pct` and `height_pct` to provide additional flexiblity with layout. Note there is no protection against a bins width/height going negative. Things may render backwards or upside down :) in that case.

# Version 0.8.1 (June 19th, 2020)

- Fix issue with cursor not being grabbed.

# Version 0.8.0 (June 16th, 2020)

- **BREAKING** Update Ilmenite (0.0 to 0.1)
- Added `should_recreate_swapchain` method to `Basalt` for non-app_loop applications.
- Added `window_type` method to `BasaltWindow` trait.
- Added `composite_alpha` method to options to allow app_loops to specify composite alpha type.
- Fixed issue where window would not be closed after calling `Basalt::exit()`.
- Fixed issue in bin hooks where holding a key down caused all hooks to be called.

# Version 0.7.0 (June 4th, 2020)

- **BREAKING** Reworked initialization so that the window event loop is on the main thread. See examples for new init procedure.
- **BREAKING** Removed xinput2 bindings.
- **BREAKING** Removed harfbuzz bindings.
- **BREAKING** Removed `do_every` hook.
- **BREAKING** Various dependency updates.
- **BREAKING** Rename `BinStyle` `position_t` field to `position` and the enum from `PositionTy` to `BinPosition`
- **BREAKING** Replaced harfbuzz/freetype text with Ilmentite. Various `BinStyle` field types have changed as result.
- Removed freetype-sys dependency.
- Replaced decorum with the more popular ordered_float.
- Resolve warnings on latest nightly.
- Fixed color format mismatches when using multisampling. (resolves debug_assert on debug builds)
- Atlas now has `batch_cache_id` method for looking up multiple cache_id coords at once.
- Added option `prefer_integrated_gpu()` which will prefer integrated graphics where possible. Dedicated graphics will be preferred otherwise.
- Added option `interface_limit_draw()` which defaults to being enabled that limits interface redrawing.
- Added option `use_exclusive_fullscreen()` which defaults to disabled that will use exclusive fullscreen instead of a borderless window.
- Added `Bin` fields `pos_from_b_offset` and `pos_from_r_offset` to allow most flexibility with percent based dimensions.
- Added support for secondary graphics, compute, and transfer queues. The related functions will return options as they are not guaranteed to be present.

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
