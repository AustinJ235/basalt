# Unreleased

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
