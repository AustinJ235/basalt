pub mod color;
pub mod style;
mod text_state;

use std::any::Any;
use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;
use std::ops::{AddAssign, DivAssign};
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Barrier, Weak};
use std::time::{Duration, Instant};

use cosmic_text::fontdb::Source as FontSource;
use cosmic_text::{FontSystem, SwashCache};
use foldhash::HashMap;
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};
use quick_cache::sync::Cache;
use text_state::TextState;

use crate::Basalt;
use crate::image::{ImageCacheLifetime, ImageInfo, ImageKey, ImageMap};
use crate::input::{
    Char, InputHookCtrl, InputHookID, InputHookTarget, KeyCombo, LocalCursorState, LocalKeyState,
    MouseButton, WindowState,
};
use crate::interface::{
    BinPosition, BinStyle, BinStyleValidation, ChildFloatMode, Color, DefaultFont, ItfVertInfo,
    scale_verts,
};
use crate::interval::IntvlHookCtrl;
use crate::render::RendererMetricsLevel;
use crate::window::Window;

/// ID of a `Bin`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BinID(pub(crate) u64);

/// Information of a `Bin` after an update
///
/// ***Note:** If the `Bin` is hidden, this will reflect its state when it was last visible.*
#[derive(Clone, Default, Debug)]
pub struct BinPostUpdate {
    /// `false` if the `Bin` is hidden, the computed opacity is *zero*, or is off-screen.
    pub visible: bool,
    /// `true` if `BinStyle.position` equals `Some(BinPosition::Floating)`
    pub floating: bool,
    /// Top Left Outer Position (Includes Border)
    pub tlo: [f32; 2],
    /// Top Left Inner Position
    pub tli: [f32; 2],
    /// Bottom Left Outer Position (Includes Border)
    pub blo: [f32; 2],
    /// Bottom Left Inner Position
    pub bli: [f32; 2],
    /// Top Right Outer Position (Includes Border)
    pub tro: [f32; 2],
    /// Top Right Inner Position
    pub tri: [f32; 2],
    /// Bottom Right Outer Position (Includes Border)
    pub bro: [f32; 2],
    /// Bottom Right Inner Position
    pub bri: [f32; 2],
    /// Z-Index as displayed
    pub z_index: i16,
    /// Optimal inner bounds [MIN_X, MAX_X, MIN_Y, MAX_Y]
    pub optimal_inner_bounds: [f32; 4],
    /// Optimal inner bounds [MIN_X, MAX_X, MIN_Y, MAX_Y] (includes margin & borders)
    pub optimal_outer_bounds: [f32; 4],
    /// Bounds of the content (includes custom_verts & text) before bounds checks.
    pub content_bounds: Option<[f32; 4]>,
    /// Optimal bounds of the content. Same as `optimal_inner_bounds` but with padding included.
    pub optimal_content_bounds: [f32; 4],
    /// Target Extent (Generally Window Size)
    pub extent: [u32; 2],
    /// UI Scale Used
    pub scale: f32,
}

#[derive(Clone)]
pub(crate) struct BinPlacement {
    z: i16,
    tlwh: [f32; 4],
    bounds: [f32; 4],
    opacity: f32,
    hidden: bool,
}

#[derive(Default, Clone)]
struct BinHrchy {
    parent: Option<Weak<Bin>>,
    children: BTreeMap<BinID, Weak<Bin>>,
}

#[derive(PartialEq, Eq, Hash)]
enum InternalHookTy {
    Updated,
    UpdatedOnce,
    ChildrenAdded,
    ChildrenRemoved,
}

enum InternalHookFn {
    Updated(Box<dyn FnMut(&Arc<Bin>, &BinPostUpdate) + Send + 'static>),
    ChildrenAdded(Box<dyn FnMut(&Arc<Bin>, &Vec<Arc<Bin>>) + Send + 'static>),
    ChildrenRemoved(Box<dyn FnMut(&Arc<Bin>, &Vec<Weak<Bin>>) + Send + 'static>),
}

struct Coords {
    tlwh: [f32; 4],
}

impl Coords {
    fn new(width: f32, height: f32) -> Self {
        Self {
            tlwh: [0.0, 0.0, width, height],
        }
    }

    fn x_pct(&self, pct: f32) -> f32 {
        (self.tlwh[2] * pct) + self.tlwh[1]
    }

    fn y_pct(&self, pct: f32) -> f32 {
        (self.tlwh[3] * pct) + self.tlwh[0]
    }
}

/// Performance metrics for a `Bin` update.
#[derive(Debug, Clone, Default)]
pub struct OVDPerfMetrics {
    pub total: f32,
    pub style: f32,
    pub placement: f32,
    pub visibility: f32,
    pub back_image: f32,
    pub back_vertex: f32,
    pub text_buffer: f32,
    pub text_layout: f32,
    pub text_vertex: f32,
    pub overflow: f32,
    pub vertex_scale: f32,
    pub post_update: f32,
    pub worker_process: f32,
}

impl AddAssign for OVDPerfMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.total += rhs.total;
        self.style += rhs.style;
        self.placement += rhs.placement;
        self.visibility += rhs.visibility;
        self.back_image += rhs.back_image;
        self.back_vertex += rhs.back_vertex;
        self.text_buffer += rhs.text_buffer;
        self.text_layout += rhs.text_layout;
        self.text_vertex += rhs.text_vertex;
        self.overflow += rhs.overflow;
        self.vertex_scale += rhs.vertex_scale;
        self.post_update += rhs.post_update;
        self.worker_process += rhs.worker_process;
    }
}

impl DivAssign<f32> for OVDPerfMetrics {
    fn div_assign(&mut self, rhs: f32) {
        self.total /= rhs;
        self.style /= rhs;
        self.placement /= rhs;
        self.visibility /= rhs;
        self.back_image /= rhs;
        self.back_vertex /= rhs;
        self.text_buffer /= rhs;
        self.text_layout /= rhs;
        self.text_vertex /= rhs;
        self.overflow /= rhs;
        self.vertex_scale /= rhs;
        self.post_update /= rhs;
        self.worker_process /= rhs;
    }
}

struct MetricsState {
    inner: OVDPerfMetrics,
    start: Instant,
    last_segment: u128,
}

impl MetricsState {
    fn start() -> Self {
        Self {
            inner: Default::default(),
            start: Instant::now(),
            last_segment: 0,
        }
    }

    fn segment<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut OVDPerfMetrics, f32),
    {
        let segment = self.start.elapsed().as_micros();
        let elapsed = (segment - self.last_segment) as f32 / 1000.0;
        self.last_segment = segment;
        f(&mut self.inner, elapsed);
    }

    fn complete(mut self) -> OVDPerfMetrics {
        self.inner.total = self.start.elapsed().as_micros() as f32 / 1000.0;
        self.inner
    }
}

pub(crate) struct UpdateContext {
    pub extent: [f32; 2],
    pub scale: f32,
    pub font_system: FontSystem,
    pub glyph_cache: SwashCache,
    pub default_font: DefaultFont,
    pub metrics_level: RendererMetricsLevel,
    pub placement_cache: Arc<
        Cache<
            BinID,
            BinPlacement,
            quick_cache::UnitWeighter,
            foldhash::fast::RandomState,
            quick_cache::sync::DefaultLifecycle<BinID, BinPlacement>,
        >,
    >,
}

impl From<&Arc<Window>> for UpdateContext {
    fn from(window: &Arc<Window>) -> Self {
        let window_size = window.inner_dimensions();
        let effective_scale = window.effective_interface_scale();
        let mut font_system = FontSystem::new();
        let default_font = window.basalt_ref().interface_ref().default_font();
        let metrics_level = window.renderer_metrics_level();

        for binary_font in window.basalt_ref().interface_ref().binary_fonts() {
            font_system
                .db_mut()
                .load_font_source(FontSource::Binary(binary_font));
        }

        Self {
            extent: [window_size[0] as f32, window_size[1] as f32],
            scale: effective_scale,
            font_system,
            glyph_cache: SwashCache::new(),
            default_font,
            metrics_level,
            placement_cache: Arc::new(Cache::with_options(
                quick_cache::OptionsBuilder::new()
                    .estimated_items_capacity(10000)
                    .weight_capacity(10000)
                    .build()
                    .unwrap(),
                quick_cache::UnitWeighter,
                foldhash::fast::RandomState::default(),
                quick_cache::sync::DefaultLifecycle::default(),
            )),
        }
    }
}

impl From<&UpdateContext> for UpdateContext {
    fn from(other: &Self) -> Self {
        let locale = other.font_system.locale().to_string();
        let db = other.font_system.db().clone();
        let font_system = FontSystem::new_with_locale_and_db(locale, db);

        Self {
            extent: other.extent,
            scale: other.scale,
            font_system,
            glyph_cache: SwashCache::new(),
            default_font: other.default_font.clone(),
            metrics_level: other.metrics_level,
            placement_cache: other.placement_cache.clone(),
        }
    }
}

#[derive(Default)]
struct UpdateState {
    text: TextState,
    back_image_info: Option<(ImageKey, ImageInfo)>,
}

/// Fundamental UI component.
pub struct Bin {
    basalt: Arc<Basalt>,
    id: BinID,
    associated_window: Mutex<Option<Weak<Window>>>,
    hrchy: RwLock<BinHrchy>,
    style: RwLock<Arc<BinStyle>>,
    initial: AtomicBool,
    post_update: RwLock<BinPostUpdate>,
    update_state: Mutex<UpdateState>,
    input_hook_ids: Mutex<Vec<InputHookID>>,
    keep_alive_objects: Mutex<Vec<Box<dyn Any + Send + Sync + 'static>>>,
    internal_hooks: Mutex<HashMap<InternalHookTy, Vec<InternalHookFn>>>,
}

impl PartialEq for Bin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.basalt, &other.basalt) && self.id == other.id
    }
}

impl Eq for Bin {}

impl std::fmt::Debug for Bin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bin").field(&self.id.0).finish()
    }
}

impl Drop for Bin {
    fn drop(&mut self) {
        for hook in self.input_hook_ids.lock().split_off(0) {
            self.basalt.input_ref().remove_hook(hook);
        }

        if let Some(parent) = self.parent() {
            let mut parent_hrchy = parent.hrchy.write();
            parent_hrchy.children.remove(&self.id);
        }

        if let Some(window) = self.window() {
            window.dissociate_bin(self.id);
        }
    }
}

impl Bin {
    pub(crate) fn new(id: BinID, basalt: Arc<Basalt>) -> Arc<Self> {
        Arc::new(Self {
            id,
            basalt,
            associated_window: Mutex::new(None),
            hrchy: RwLock::new(BinHrchy::default()),
            style: RwLock::new(Arc::new(Default::default())),
            initial: AtomicBool::new(true),
            post_update: RwLock::new(Default::default()),
            update_state: Mutex::new(Default::default()),
            input_hook_ids: Mutex::new(Vec::new()),
            keep_alive_objects: Mutex::new(Vec::new()),
            internal_hooks: Mutex::new(HashMap::from_iter([
                (InternalHookTy::Updated, Vec::new()),
                (InternalHookTy::UpdatedOnce, Vec::new()),
                (InternalHookTy::ChildrenAdded, Vec::new()),
                (InternalHookTy::ChildrenRemoved, Vec::new()),
            ])),
        })
    }

    /// Returns the `BinID` of this `Bin`.
    pub fn id(&self) -> BinID {
        self.id
    }

    /// Obtain a copy of `Arc<Basalt>`
    pub fn basalt(&self) -> Arc<Basalt> {
        self.basalt.clone()
    }

    /// Obtain a reference of `Arc<Basalt>`
    pub fn basalt_ref(&self) -> &Arc<Basalt> {
        &self.basalt
    }

    /// Obtain the currently associated `Arc<Window>`.
    ///
    /// Returns `None` when there is no window associated.
    pub fn window(&self) -> Option<Arc<Window>> {
        self.associated_window
            .lock()
            .clone()
            .and_then(|weak| weak.upgrade())
    }

    /// Change window association of this `Bin`.
    ///
    /// ***Note**: This does not effect any of its children. If that is desired use the
    /// `associate_window_recursive` method instead.*
    pub fn associate_window(self: &Arc<Self>, window: &Arc<Window>) {
        let mut associated_window = self.associated_window.lock();

        if let Some(old_window) = associated_window.take().and_then(|wk| wk.upgrade()) {
            old_window.dissociate_bin(self.id);
        }

        window.associate_bin(self.clone());
        *associated_window = Some(Arc::downgrade(window));
    }

    /// Change window association of this `Bin` and all of its children recursively.
    pub fn associate_window_recursive(self: &Arc<Self>, window: &Arc<Window>) {
        for bin in self.children_recursive_with_self() {
            bin.associate_window(window);
        }
    }

    /// Return the parent of this `Bin`.
    pub fn parent(&self) -> Option<Arc<Bin>> {
        self.hrchy
            .read()
            .parent
            .as_ref()
            .and_then(|parent_wk| parent_wk.upgrade())
    }

    /// Return the ancestors of this `Bin` where the order is from parent, parent's
    /// parent, parent's parent's parent, etc...
    pub fn ancestors(&self) -> Vec<Arc<Bin>> {
        let mut ancestors = match self.parent() {
            Some(parent) => vec![parent],
            None => return Vec::new(),
        };

        while let Some(parent) = ancestors.last().unwrap().parent() {
            ancestors.push(parent);
        }

        ancestors
    }

    /// Return the children of this `Bin`
    pub fn children(&self) -> Vec<Arc<Bin>> {
        self.hrchy
            .read()
            .children
            .iter()
            .filter_map(|(_, child_wk)| child_wk.upgrade())
            .collect()
    }

    /// Return the children of this `Bin` recursively.
    ///
    /// ***Note:** There is no order to the result.*
    pub fn children_recursive(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let mut children = self.children();
        let mut i = 0;

        while i < children.len() {
            let child = children[i].clone();

            children.extend(
                child
                    .hrchy
                    .read()
                    .children
                    .iter()
                    .filter_map(|(_, child_wk)| child_wk.upgrade()),
            );

            i += 1;
        }

        children
    }

    /// Return the children of this `Bin` recursively including itself.
    ///
    /// ***Note:** There is no order to the result.*
    pub fn children_recursive_with_self(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let mut children = vec![self.clone()];
        let mut i = 0;

        while i < children.len() {
            let child = children[i].clone();

            children.extend(
                child
                    .hrchy
                    .read()
                    .children
                    .iter()
                    .filter_map(|(_, child_wk)| child_wk.upgrade()),
            );

            i += 1;
        }

        children
    }

    /// Add a child to this `Bin`.
    pub fn add_child(self: &Arc<Self>, child: Arc<Bin>) {
        child.hrchy.write().parent = Some(Arc::downgrade(self));

        self.hrchy
            .write()
            .children
            .insert(child.id, Arc::downgrade(&child));

        child.trigger_recursive_update();
        self.call_children_added_hooks(vec![child]);
    }

    /// Add multiple children to this `Bin`.
    pub fn add_children(self: &Arc<Self>, children: Vec<Arc<Bin>>) {
        for child in children.iter() {
            child.hrchy.write().parent = Some(Arc::downgrade(self));
        }

        self.hrchy.write().children.extend(
            children
                .iter()
                .map(|child| (child.id, Arc::downgrade(child))),
        );

        children
            .iter()
            .for_each(|child| child.trigger_recursive_update());

        self.call_children_added_hooks(children);
    }

    /// Take the children from this `Bin`.
    pub fn take_children(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let mut weak_map = BTreeMap::new();
        std::mem::swap(&mut self.hrchy.write().children, &mut weak_map);
        let children_wk = weak_map.into_values().collect::<Vec<_>>();

        let children = children_wk
            .iter()
            .filter_map(|child_wk| child_wk.upgrade())
            .collect::<Vec<_>>();

        for child in children.iter() {
            child.hrchy.write().parent = None;
        }

        self.call_children_removed_hooks(children_wk);

        for child in children.iter() {
            child.trigger_recursive_update();
        }

        children
    }

    /// Obtain an `Arc` of `BinStyle` of this `Bin`.
    ///
    /// This is useful where it is only needed to inspect the style of the `Bin`.
    pub fn style(&self) -> Arc<BinStyle> {
        self.style.read().clone()
    }

    /// Obtain a copy of `BinStyle`  of this `Bin`.
    pub fn style_copy(&self) -> BinStyle {
        (**self.style.read()).clone()
    }

    /// Inspect `BinStyle` by reference given a method.
    ///
    /// When inspecting a style where it is only needed for a short period of time, this method
    /// will avoid cloning an `Arc` in comparision to the `style` method.
    pub fn style_inspect<F: FnMut(&BinStyle) -> T, T>(&self, mut method: F) -> T {
        method(&self.style.read())
    }

    /// Update the style of this `Bin`.
    ///
    /// ***Note:** If the style has a validation error, the style will not be updated.*
    #[track_caller]
    pub fn style_update(self: &Arc<Self>, updated_style: BinStyle) -> BinStyleValidation {
        let validation = updated_style.validate(self);
        let mut effects_siblings = updated_style.position == Some(BinPosition::Floating);

        if !validation.errors_present() {
            let mut old_style = Arc::new(updated_style);
            std::mem::swap(&mut *self.style.write(), &mut old_style);

            self.initial.store(false, atomic::Ordering::SeqCst);
            effects_siblings |= old_style.position == Some(BinPosition::Floating);

            if effects_siblings {
                match self.parent() {
                    Some(parent) => parent.trigger_children_update(),
                    None => {
                        // NOTE: Parent should always be Some(_) in this case, but fallback to
                        //       a standard recursive update for robustness
                        self.trigger_recursive_update();
                    },
                }
            } else {
                self.trigger_recursive_update();
            }
        }

        validation
    }

    /// Check if this `Bin` is hidden.
    ///
    /// ***Note:** This is based on the `BinStyle.hidden` value, not if it is offscreen.*
    pub fn is_hidden(&self) -> bool {
        match self.style_inspect(|style| style.hidden) {
            Some(hidden) => hidden,
            None => {
                match self.parent() {
                    Some(parent) => parent.is_hidden(),
                    None => false,
                }
            },
        }
    }

    /// Set the `BinStyle.hidden` value.
    pub fn set_hidden(self: &Arc<Self>, hidden: Option<bool>) {
        self.style_update(BinStyle {
            hidden,
            ..self.style_copy()
        })
        .expect_valid();
    }

    /// Toggle the hidden value of this `Bin`.
    pub fn toggle_hidden(self: &Arc<Self>) {
        let mut style = self.style_copy();
        style.hidden = Some(!style.hidden.unwrap_or(false));
        self.style_update(style).expect_valid();
    }

    /// Trigger an update to happen on this `Bin`
    pub fn trigger_update(&self) {
        let window = match self.window() {
            Some(some) => some,
            None => return,
        };

        window.update_bin(self.id);
    }

    /// Trigger an update to happen on this `Bin` and its children.
    pub fn trigger_recursive_update(self: &Arc<Self>) {
        let window = match self.window() {
            Some(some) => some,
            None => return,
        };

        window.update_bin_batch(
            self.children_recursive_with_self()
                .into_iter()
                .map(|child| child.id)
                .collect(),
        );
    }

    /// Similar to `trigger_recursive_update` but doesn't trigger an update on this `Bin`.
    pub fn trigger_children_update(self: &Arc<Self>) {
        let window = match self.window() {
            Some(some) => some,
            None => return,
        };

        window.update_bin_batch(
            self.children_recursive()
                .into_iter()
                .map(|child| child.id)
                .collect(),
        );
    }

    /// Wait for an update to occur on this `Bin`.
    pub fn wait_for_update(self: &Arc<Self>) {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_copy = barrier.clone();

        self.on_update_once(move |_, _| {
            barrier_copy.wait();
        });

        barrier.wait();
    }

    /// Obtain the `BinPostUpdate` information this `Bin`.
    pub fn post_update(&self) -> BinPostUpdate {
        self.post_update.read().clone()
    }

    /// Calculate the amount of vertical overflow.
    pub fn calc_vert_overflow(self: &Arc<Bin>) -> f32 {
        let self_bpu = self.post_update.read();
        let [pad_t, pad_b] =
            self.style_inspect(|style| [style.pad_t.unwrap_or(0.0), style.pad_b.unwrap_or(0.0)]);
        let mut overflow_t: f32 = 0.0;
        let mut overflow_b: f32 = 0.0;

        for child in self.children() {
            let child_bpu = child.post_update.read();

            if child_bpu.floating {
                overflow_t = overflow_t.max(
                    (self_bpu.optimal_inner_bounds[2] + pad_t) - child_bpu.optimal_outer_bounds[2],
                );
                overflow_b = overflow_b.max(
                    child_bpu.optimal_outer_bounds[3] - (self_bpu.optimal_inner_bounds[3] - pad_b),
                );
            } else {
                overflow_t = overflow_t
                    .max(self_bpu.optimal_inner_bounds[2] - child_bpu.optimal_outer_bounds[2]);
                overflow_b = overflow_b
                    .max(child_bpu.optimal_outer_bounds[3] - self_bpu.optimal_inner_bounds[3]);
            }
        }

        // TODO: This only includes the content of this bin. Should it include others?
        if let Some(content_bounds) = self_bpu.content_bounds {
            overflow_t = overflow_t.max(self_bpu.optimal_content_bounds[2] - content_bounds[2]);
            overflow_b = overflow_b.max(content_bounds[3] - self_bpu.optimal_content_bounds[3]);
        }

        overflow_t + overflow_b
    }

    /// Calculate the amount of horizontal overflow.
    pub fn calc_hori_overflow(self: &Arc<Bin>) -> f32 {
        let self_bpu = self.post_update.read();
        let [pad_l, pad_r] =
            self.style_inspect(|style| [style.pad_l.unwrap_or(0.0), style.pad_r.unwrap_or(0.0)]);
        let mut overflow_l: f32 = 0.0;
        let mut overflow_r: f32 = 0.0;

        for child in self.children() {
            let child_bpu = child.post_update.read();

            if child_bpu.floating {
                overflow_l = overflow_l.max(
                    (self_bpu.optimal_inner_bounds[0] + pad_l) - child_bpu.optimal_outer_bounds[0],
                );
                overflow_r = overflow_r.max(
                    child_bpu.optimal_outer_bounds[1] - (self_bpu.optimal_inner_bounds[1] - pad_r),
                );
            } else {
                overflow_l = overflow_l
                    .max(self_bpu.optimal_inner_bounds[0] - child_bpu.optimal_outer_bounds[0]);
                overflow_r = overflow_r
                    .max(child_bpu.optimal_outer_bounds[1] - self_bpu.optimal_inner_bounds[1]);
            }
        }

        // TODO: This only includes the content of this bin. Should it include others?
        if let Some(content_bounds) = self_bpu.content_bounds {
            overflow_l = overflow_l.max(self_bpu.optimal_content_bounds[0] - content_bounds[0]);
            overflow_r = overflow_r.max(content_bounds[1] - self_bpu.optimal_content_bounds[1]);
        }

        overflow_l + overflow_r
    }

    /// Check if the mouse is inside of this `Bin`.
    ///
    /// ***Note:** This does not check the window.*
    pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
        let post = self.post_update.read();

        if !post.visible {
            return false;
        }

        if mouse_x >= post.tlo[0]
            && mouse_x <= post.tro[0]
            && mouse_y >= post.tlo[1]
            && mouse_y <= post.blo[1]
        {
            return true;
        }

        false
    }

    /// Keep objects alive for the lifetime of the `Bin`.
    pub fn keep_alive<O, T>(&self, objects: O)
    where
        O: IntoIterator<Item = T>,
        T: Any + Send + Sync + 'static,
    {
        for object in objects {
            self.keep_alive_objects.lock().push(Box::new(object));
        }
    }

    pub fn add_enter_text_events(self: &Arc<Self>) {
        self.on_character(move |target, _, c| {
            let this = target.into_bin().unwrap();
            let mut style = this.style_copy();
            c.modify_string(&mut style.text);
            this.style_update(style).expect_valid();
            Default::default()
        });
    }

    pub fn add_drag_events(self: &Arc<Self>, target_op: Option<Arc<Bin>>) {
        let window = match self.window() {
            Some(some) => some,
            None => return,
        };

        #[derive(Default)]
        struct Data {
            target: Weak<Bin>,
            mouse_x: f32,
            mouse_y: f32,
            pos_from_t: Option<f32>,
            pos_from_b: Option<f32>,
            pos_from_l: Option<f32>,
            pos_from_r: Option<f32>,
        }

        let data = Arc::new(Mutex::new(None));
        let target_wk = target_op
            .map(|v| Arc::downgrade(&v))
            .unwrap_or_else(|| Arc::downgrade(self));
        let data_cp = data.clone();

        self.on_press(MouseButton::Middle, move |_, window, _| {
            let [mouse_x, mouse_y] = window.cursor_pos();

            let style = match target_wk.upgrade() {
                Some(bin) => bin.style_copy(),
                None => return InputHookCtrl::Remove,
            };

            *data_cp.lock() = Some(Data {
                target: target_wk.clone(),
                mouse_x,
                mouse_y,
                pos_from_t: style.pos_from_t,
                pos_from_b: style.pos_from_b,
                pos_from_l: style.pos_from_l,
                pos_from_r: style.pos_from_r,
            });

            Default::default()
        });

        let data_cp = data.clone();

        self.attach_input_hook(
            self.basalt
                .input_ref()
                .hook()
                .window(&window)
                .on_cursor()
                .call(move |_, window, _| {
                    let [mouse_x, mouse_y] = window.cursor_pos();
                    let mut data_op = data_cp.lock();

                    let data = match &mut *data_op {
                        Some(some) => some,
                        None => return Default::default(),
                    };

                    let target = match data.target.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    let dx = mouse_x - data.mouse_x;
                    let dy = mouse_y - data.mouse_y;

                    target
                        .style_update(BinStyle {
                            pos_from_t: data.pos_from_t.as_ref().map(|v| *v + dy),
                            pos_from_b: data.pos_from_b.as_ref().map(|v| *v - dy),
                            pos_from_l: data.pos_from_l.as_ref().map(|v| *v + dx),
                            pos_from_r: data.pos_from_r.as_ref().map(|v| *v - dx),
                            ..target.style_copy()
                        })
                        .expect_valid();

                    target.trigger_children_update();
                    Default::default()
                })
                .finish()
                .unwrap(),
        );

        self.on_release(MouseButton::Middle, move |_, _, _| {
            *data.lock() = None;
            Default::default()
        });
    }

    pub fn fade_out(self: &Arc<Self>, millis: u64) {
        let bin_wk = Arc::downgrade(self);
        let start_opacity = self.style_copy().opacity.unwrap_or(1.0);
        let steps = (millis / 8) as i64;
        let step_size = start_opacity / steps as f32;
        let mut step_i = 0;

        self.basalt
            .interval_ref()
            .do_every(Duration::from_millis(8), None, move |_| {
                if step_i > steps {
                    return IntvlHookCtrl::Remove;
                }

                let bin = match bin_wk.upgrade() {
                    Some(some) => some,
                    None => return IntvlHookCtrl::Remove,
                };

                let opacity = start_opacity - (step_i as f32 * step_size);
                let mut copy = bin.style_copy();
                copy.opacity = Some(opacity);

                if step_i == steps {
                    copy.hidden = Some(true);
                }

                bin.style_update(copy).expect_valid();
                bin.trigger_children_update();
                step_i += 1;
                Default::default()
            });
    }

    pub fn fade_in(self: &Arc<Self>, millis: u64, target: f32) {
        let bin_wk = Arc::downgrade(self);
        let start_opacity = self.style_copy().opacity.unwrap_or(1.0);
        let steps = (millis / 8) as i64;
        let step_size = (target - start_opacity) / steps as f32;
        let mut step_i = 0;

        self.basalt
            .interval_ref()
            .do_every(Duration::from_millis(8), None, move |_| {
                if step_i > steps {
                    return IntvlHookCtrl::Remove;
                }

                let bin = match bin_wk.upgrade() {
                    Some(some) => some,
                    None => return IntvlHookCtrl::Remove,
                };

                let opacity = (step_i as f32 * step_size) + start_opacity;
                let mut copy = bin.style_copy();
                copy.opacity = Some(opacity);
                copy.hidden = Some(false);
                bin.style_update(copy).expect_valid();
                bin.trigger_children_update();
                step_i += 1;
                Default::default()
            });
    }

    /// Attach an `InputHookID` to this `Bin`. When this `Bin` drops the hook will be removed.
    pub fn attach_input_hook(&self, hook_id: InputHookID) {
        self.input_hook_ids.lock().push(hook_id);
    }

    pub fn on_press<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_press()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_release<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalKeyState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_release()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_hold<C: KeyCombo, F>(self: &Arc<Self>, combo: C, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &LocalKeyState, Option<Duration>) -> InputHookCtrl
            + Send
            + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_hold()
            .keys(combo)
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_character<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, Char) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_character()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_enter<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_enter()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_leave<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_leave()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_focus<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_focus()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_focus_lost<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_focus_lost()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_scroll<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, f32, f32) -> InputHookCtrl + Send + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_scroll()
            .call(method)
            .finish()
            .unwrap()
    }

    pub fn on_cursor<F>(self: &Arc<Self>, method: F) -> InputHookID
    where
        F: FnMut(InputHookTarget, &WindowState, &LocalCursorState) -> InputHookCtrl
            + Send
            + 'static,
    {
        self.basalt
            .input_ref()
            .hook()
            .bin(self)
            .on_cursor()
            .call(method)
            .finish()
            .unwrap()
    }

    #[inline]
    pub fn on_children_added<F: FnMut(&Arc<Bin>, &Vec<Arc<Bin>>) + Send + 'static>(
        self: &Arc<Self>,
        func: F,
    ) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::ChildrenAdded)
            .unwrap()
            .push(InternalHookFn::ChildrenAdded(Box::new(func)));
    }

    #[inline]
    pub fn on_children_removed<F: FnMut(&Arc<Bin>, &Vec<Weak<Bin>>) + Send + 'static>(
        self: &Arc<Self>,
        func: F,
    ) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::ChildrenRemoved)
            .unwrap()
            .push(InternalHookFn::ChildrenRemoved(Box::new(func)));
    }

    #[inline]
    pub fn on_update<F: FnMut(&Arc<Bin>, &BinPostUpdate) + Send + 'static>(
        self: &Arc<Self>,
        func: F,
    ) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::Updated)
            .unwrap()
            .push(InternalHookFn::Updated(Box::new(func)));
    }

    #[inline]
    pub fn on_update_once<F: FnMut(&Arc<Bin>, &BinPostUpdate) + Send + 'static>(
        self: &Arc<Self>,
        func: F,
    ) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::UpdatedOnce)
            .unwrap()
            .push(InternalHookFn::Updated(Box::new(func)));
    }

    fn call_children_added_hooks(self: &Arc<Self>, children: Vec<Arc<Bin>>) {
        for func_enum in self
            .internal_hooks
            .lock()
            .get_mut(&InternalHookTy::ChildrenAdded)
            .unwrap()
            .iter_mut()
        {
            if let InternalHookFn::ChildrenAdded(func) = func_enum {
                func(self, &children);
            }
        }
    }

    fn call_children_removed_hooks(self: &Arc<Self>, children: Vec<Weak<Bin>>) {
        for func_enum in self
            .internal_hooks
            .lock()
            .get_mut(&InternalHookTy::ChildrenRemoved)
            .unwrap()
            .iter_mut()
        {
            if let InternalHookFn::ChildrenRemoved(func) = func_enum {
                func(self, &children);
            }
        }
    }

    fn calc_placement(&self, context: &mut UpdateContext) -> BinPlacement {
        if let Some(placement) = context.placement_cache.get(&self.id) {
            return placement;
        }

        let extent = [
            context.extent[0] / context.scale,
            context.extent[1] / context.scale,
        ];

        if self.initial.load(atomic::Ordering::SeqCst) {
            return BinPlacement {
                z: 0,
                tlwh: [0.0, 0.0, extent[0], extent[1]],
                bounds: [0.0, extent[0], 0.0, extent[1]],
                opacity: 1.0,
                hidden: false,
            };
        }

        let style = self.style();
        let position = style.position.unwrap_or(BinPosition::Window);

        if position == BinPosition::Floating {
            let parent = self.parent().unwrap();
            let parent_plmt = parent.calc_placement(context);

            let (padding_tblr, scroll_xy, float_mode) = {
                let parent_style = parent.style();

                (
                    [
                        parent_style.pad_t.unwrap_or(0.0),
                        parent_style.pad_b.unwrap_or(0.0),
                        parent_style.pad_l.unwrap_or(0.0),
                        parent_style.pad_r.unwrap_or(0.0),
                    ],
                    [
                        parent_style.scroll_x.unwrap_or(0.0),
                        parent_style.scroll_y.unwrap_or(0.0),
                    ],
                    parent_style.child_float_mode.unwrap_or(ChildFloatMode::Row),
                )
            };

            let body_width = parent_plmt.tlwh[2] - padding_tblr[2] - padding_tblr[3];
            let body_height = parent_plmt.tlwh[3] - padding_tblr[0] - padding_tblr[1];

            struct Sibling {
                this: bool,
                weight: i16,
                size_xy: [f32; 2],
                margin_tblr: [f32; 4],
            }

            let mut siblings = parent
                .children()
                .into_iter()
                .enumerate()
                .filter_map(|(i, sibling)| {
                    let sibling_style = sibling.style();

                    // TODO: Ignore if hidden?
                    if sibling_style.position != Some(BinPosition::Floating) {
                        return None;
                    }

                    let width = match sibling_style.width {
                        Some(width) => width,
                        None => {
                            match sibling_style.width_pct {
                                Some(width_pct) => width_pct * body_width,
                                None => unreachable!(),
                            }
                        },
                    } + sibling_style.width_offset.unwrap_or(0.0);

                    let height = match sibling_style.height {
                        Some(height) => height,
                        None => {
                            match sibling_style.height_pct {
                                Some(height_pct) => height_pct * body_height,
                                None => unreachable!(),
                            }
                        },
                    } + sibling_style.height_offset.unwrap_or(0.0);

                    Some(Sibling {
                        this: sibling.id == self.id,
                        weight: sibling_style.float_weight.unwrap_or(i as i16),
                        size_xy: [width, height],
                        margin_tblr: [
                            sibling_style.margin_t.unwrap_or(0.0),
                            sibling_style.margin_b.unwrap_or(0.0),
                            sibling_style.margin_l.unwrap_or(0.0),
                            sibling_style.margin_r.unwrap_or(0.0),
                        ],
                    })
                })
                .collect::<Vec<_>>();

            siblings.sort_by_key(|sibling| sibling.weight);

            let z = match style.z_index {
                Some(z) => z,
                None => parent_plmt.z + 1,
            } + style.add_z_index.unwrap_or(0);

            let opacity = match style.opacity {
                Some(opacity) => parent_plmt.opacity * opacity,
                None => parent_plmt.opacity,
            };

            let hidden = match style.hidden {
                Some(hidden) => hidden,
                None => parent_plmt.hidden,
            };

            match float_mode {
                ChildFloatMode::Row => {
                    let mut x = 0.0;
                    let mut y = 0.0;
                    let mut row_height = 0.0;
                    let mut row_bins = 0;

                    for sibling in siblings {
                        if sibling.this {
                            let effective_width = sibling.size_xy[0]
                                + sibling.margin_tblr[2]
                                + sibling.margin_tblr[3];

                            if x + effective_width > body_width && row_bins != 0 {
                                x = 0.0;
                                y += row_height;
                            }

                            let top =
                                parent_plmt.tlwh[0] + y + padding_tblr[0] + sibling.margin_tblr[0]
                                    - scroll_xy[1];
                            let left = parent_plmt.tlwh[1]
                                + x
                                + padding_tblr[2]
                                + sibling.margin_tblr[2]
                                + scroll_xy[0];
                            let [width, height] = sibling.size_xy;

                            let x_bounds = match style.overflow_x.unwrap_or(false) {
                                true => [parent_plmt.bounds[0], parent_plmt.bounds[1]],
                                false => {
                                    [
                                        left.max(parent_plmt.bounds[0]),
                                        (left + width).min(parent_plmt.bounds[1]),
                                    ]
                                },
                            };

                            let y_bounds = match style.overflow_y.unwrap_or(false) {
                                true => [parent_plmt.bounds[2], parent_plmt.bounds[3]],
                                false => {
                                    [
                                        top.max(parent_plmt.bounds[2]),
                                        (top + height).min(parent_plmt.bounds[3]),
                                    ]
                                },
                            };

                            return BinPlacement {
                                z,
                                tlwh: [top, left, width, height],
                                bounds: [x_bounds[0], x_bounds[1], y_bounds[0], y_bounds[1]],
                                opacity,
                                hidden,
                            };
                        } else {
                            let effective_width = sibling.size_xy[0]
                                + sibling.margin_tblr[2]
                                + sibling.margin_tblr[3];
                            let effective_height = sibling.size_xy[1]
                                + sibling.margin_tblr[0]
                                + sibling.margin_tblr[1];

                            if x + effective_width > body_width {
                                if row_bins == 0 {
                                    y += effective_height;
                                } else {
                                    x = effective_width;
                                    y += row_height;
                                    row_height = effective_height;
                                    row_bins = 1;
                                }
                            } else {
                                x += effective_width;
                                row_height = row_height.max(effective_height);
                                row_bins += 1;
                            }
                        }
                    }
                },
                ChildFloatMode::Column => {
                    let mut x = 0.0;
                    let mut y = 0.0;
                    let mut col_width = 0.0;
                    let mut col_bins = 0;

                    for sibling in siblings {
                        if sibling.this {
                            let effective_height = sibling.size_xy[1]
                                + sibling.margin_tblr[0]
                                + sibling.margin_tblr[1];

                            if y + effective_height > body_height && col_bins != 0 {
                                y = 0.0;
                                x += col_width;
                            }

                            let top =
                                parent_plmt.tlwh[0] + y + padding_tblr[0] + sibling.margin_tblr[0]
                                    - scroll_xy[1];
                            let left = parent_plmt.tlwh[1]
                                + x
                                + padding_tblr[2]
                                + sibling.margin_tblr[2]
                                + scroll_xy[0];
                            let [width, height] = sibling.size_xy;

                            let x_bounds = match style.overflow_x.unwrap_or(false) {
                                true => [parent_plmt.bounds[0], parent_plmt.bounds[1]],
                                false => {
                                    [
                                        left.max(parent_plmt.bounds[0]),
                                        (left + width).min(parent_plmt.bounds[1]),
                                    ]
                                },
                            };

                            let y_bounds = match style.overflow_y.unwrap_or(false) {
                                true => [parent_plmt.bounds[2], parent_plmt.bounds[3]],
                                false => {
                                    [
                                        top.max(parent_plmt.bounds[2]),
                                        (top + height).min(parent_plmt.bounds[3]),
                                    ]
                                },
                            };

                            return BinPlacement {
                                z,
                                tlwh: [top, left, width, height],
                                bounds: [x_bounds[0], x_bounds[1], y_bounds[0], y_bounds[1]],
                                opacity,
                                hidden,
                            };
                        } else {
                            let effective_width = sibling.size_xy[0]
                                + sibling.margin_tblr[2]
                                + sibling.margin_tblr[3];
                            let effective_height = sibling.size_xy[1]
                                + sibling.margin_tblr[0]
                                + sibling.margin_tblr[1];

                            if y + effective_height > body_height {
                                if col_bins == 0 {
                                    x += effective_width;
                                } else {
                                    y = effective_height;
                                    x += col_width;
                                    col_width = effective_width;
                                    col_bins = 1;
                                }
                            } else {
                                y += effective_height;
                                col_width = col_width.max(effective_width);
                                col_bins += 1;
                            }
                        }
                    }
                },
            }

            unreachable!()
        }

        let (parent_plmt, scroll_xy) = match position {
            BinPosition::Floating => unreachable!(),
            BinPosition::Window => {
                (
                    BinPlacement {
                        z: 0,
                        tlwh: [0.0, 0.0, extent[0], extent[1]],
                        bounds: [0.0, extent[0], 0.0, extent[1]],
                        opacity: 1.0,
                        hidden: false,
                    },
                    [0.0; 2],
                )
            },
            BinPosition::Parent => {
                self.parent()
                    .map(|parent| {
                        (
                            parent.calc_placement(context),
                            parent.style_inspect(|style| {
                                [style.scroll_x.unwrap_or(0.0), style.scroll_y.unwrap_or(0.0)]
                            }),
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            BinPlacement {
                                z: 0,
                                tlwh: [0.0, 0.0, extent[0], extent[1]],
                                bounds: [0.0, extent[0], 0.0, extent[1]],
                                opacity: 1.0,
                                hidden: false,
                            },
                            [0.0; 2],
                        )
                    })
            },
        };

        let top_op = match style.pos_from_t {
            Some(top) => Some(top),
            None => {
                style
                    .pos_from_t_pct
                    .map(|top_pct| (top_pct / 100.0) * parent_plmt.tlwh[3])
            },
        }
        .map(|top| top + style.pos_from_t_offset.unwrap_or(0.0));

        let bottom_op = match style.pos_from_b {
            Some(bottom) => Some(bottom),
            None => {
                style
                    .pos_from_b_pct
                    .map(|bottom_pct| (bottom_pct / 100.0) * parent_plmt.tlwh[3])
            },
        }
        .map(|bottom| bottom + style.pos_from_b_offset.unwrap_or(0.0));

        let left_op = match style.pos_from_l {
            Some(left) => Some(left),
            None => {
                style
                    .pos_from_l_pct
                    .map(|left_pct| (left_pct / 100.0) * parent_plmt.tlwh[2])
            },
        }
        .map(|left| left + style.pos_from_l_offset.unwrap_or(0.0));

        let right_op = match style.pos_from_r {
            Some(right) => Some(right),
            None => {
                style
                    .pos_from_r_pct
                    .map(|right_pct| (right_pct / 100.0) * parent_plmt.tlwh[2])
            },
        }
        .map(|right| right + style.pos_from_r_offset.unwrap_or(0.0));

        let width_op = match style.width {
            Some(width) => Some(width),
            None => {
                style
                    .width_pct
                    .map(|width_pct| (width_pct / 100.0) * parent_plmt.tlwh[2])
            },
        }
        .map(|width| width + style.width_offset.unwrap_or(0.0));

        let height_op = match style.height {
            Some(height) => Some(height),
            None => {
                style
                    .height_pct
                    .map(|height_pct| (height_pct / 100.0) * parent_plmt.tlwh[3])
            },
        }
        .map(|height| height + style.height_offset.unwrap_or(0.0));

        let [top, height] = match (top_op, bottom_op, height_op) {
            (Some(top), _, Some(height)) => [parent_plmt.tlwh[0] + top - scroll_xy[1], height],
            (_, Some(bottom), Some(height)) => {
                [
                    parent_plmt.tlwh[0] + parent_plmt.tlwh[3] - bottom - height - scroll_xy[1],
                    height,
                ]
            },
            (Some(top), Some(bottom), _) => {
                let top = parent_plmt.tlwh[0] + top + scroll_xy[1];
                let bottom = parent_plmt.tlwh[0] + parent_plmt.tlwh[3] - bottom - scroll_xy[1];
                [top, bottom - top + style.height_offset.unwrap_or(0.0)]
            },
            _ => panic!("invalid style"),
        };

        let [left, width] = match (left_op, right_op, width_op) {
            (Some(left), _, Some(width)) => [parent_plmt.tlwh[1] + left + scroll_xy[0], width],
            (_, Some(right), Some(width)) => {
                [
                    parent_plmt.tlwh[1] + parent_plmt.tlwh[2] - right - width + scroll_xy[0],
                    width,
                ]
            },
            (Some(left), Some(right), _) => {
                let left = parent_plmt.tlwh[1] + left + scroll_xy[0];
                let right = parent_plmt.tlwh[1] + parent_plmt.tlwh[2] - right + scroll_xy[0];
                [left, right - left + style.width_offset.unwrap_or(0.0)]
            },
            _ => panic!("invalid style"),
        };

        let z = match style.z_index {
            Some(z) => z,
            None => parent_plmt.z + 1,
        } + style.add_z_index.unwrap_or(0);

        let x_bounds = match style.overflow_x.unwrap_or(false) {
            true => [parent_plmt.bounds[0], parent_plmt.bounds[1]],
            false => {
                [
                    left.max(parent_plmt.bounds[0]),
                    (left + width).min(parent_plmt.bounds[1]),
                ]
            },
        };

        let y_bounds = match style.overflow_y.unwrap_or(false) {
            true => [parent_plmt.bounds[2], parent_plmt.bounds[3]],
            false => {
                [
                    top.max(parent_plmt.bounds[2]),
                    (top + height).min(parent_plmt.bounds[3]),
                ]
            },
        };

        let opacity = match style.opacity {
            Some(opacity) => parent_plmt.opacity * opacity,
            None => parent_plmt.opacity,
        };

        let hidden = match style.hidden {
            Some(hidden) => hidden,
            None => parent_plmt.hidden,
        };

        let placement = BinPlacement {
            z,
            tlwh: [top, left, width, height],
            bounds: [x_bounds[0], x_bounds[1], y_bounds[0], y_bounds[1]],
            opacity,
            hidden,
        };

        context.placement_cache.insert(self.id, placement.clone());
        placement
    }

    fn call_on_update_hooks(self: &Arc<Self>, bpu: &BinPostUpdate) {
        let mut internal_hooks = self.internal_hooks.lock();

        for hook_enum in internal_hooks
            .get_mut(&InternalHookTy::Updated)
            .unwrap()
            .iter_mut()
        {
            if let InternalHookFn::Updated(func) = hook_enum {
                func(self, bpu);
            }
        }

        for hook_enum in internal_hooks
            .get_mut(&InternalHookTy::UpdatedOnce)
            .unwrap()
            .drain(..)
        {
            if let InternalHookFn::Updated(mut func) = hook_enum {
                func(self, bpu);
            }
        }
    }

    pub(crate) fn obtain_vertex_data(
        self: &Arc<Self>,
        context: &mut UpdateContext,
    ) -> (ImageMap<Vec<ItfVertInfo>>, Option<OVDPerfMetrics>) {
        let mut metrics_op = if context.metrics_level == RendererMetricsLevel::Full {
            Some(MetricsState::start())
        } else {
            None
        };

        // -- Update Check ------------------------------------------------------------------ //

        if self.initial.load(atomic::Ordering::SeqCst) {
            return (ImageMap::new(), None);
        }

        // -- Obtain BinPostUpdate & Style --------------------------------------------------- //

        let mut bpu = self.post_update.write();
        let mut update_state = self.update_state.lock();
        let style = self.style();

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.style = elapsed;
            });
        }

        // -- Placement Calculation ---------------------------------------------------------- //

        let BinPlacement {
            z: z_index,
            tlwh,
            bounds: inner_bounds,
            opacity,
            hidden,
        } = self.calc_placement(context);

        // -- Update BinPostUpdate ----------------------------------------------------------- //

        let [top, left, width, height] = tlwh;
        let border_size_t = style.border_size_t.unwrap_or(0.0);
        let border_size_b = style.border_size_b.unwrap_or(0.0);
        let border_size_l = style.border_size_l.unwrap_or(0.0);
        let border_size_r = style.border_size_r.unwrap_or(0.0);
        let margin_t = style.margin_t.unwrap_or(0.0);
        let margin_b = style.margin_b.unwrap_or(0.0);
        let margin_l = style.margin_l.unwrap_or(0.0);
        let margin_r = style.margin_r.unwrap_or(0.0);
        let pad_t = style.pad_t.unwrap_or(0.0);
        let pad_b = style.pad_b.unwrap_or(0.0);
        let pad_l = style.pad_l.unwrap_or(0.0);
        let pad_r = style.pad_r.unwrap_or(0.0);
        let base_z = z_unorm(z_index);
        let content_z = z_unorm(z_index + 1);

        let outer_bounds = [
            inner_bounds[0] - border_size_l,
            inner_bounds[1] + border_size_r,
            inner_bounds[2] - border_size_t,
            inner_bounds[3] + border_size_b,
        ];

        *bpu = BinPostUpdate {
            visible: true,
            floating: style.position == Some(BinPosition::Floating),
            tlo: [left - border_size_l, top - border_size_t],
            tli: [left, top],
            blo: [left - border_size_l, top + height + border_size_b],
            bli: [left, top + height],
            tro: [left + width + border_size_r, top - border_size_t],
            tri: [left + width, top],
            bro: [left + width + border_size_r, top + height + border_size_b],
            bri: [left + width, top + height],
            z_index,
            optimal_inner_bounds: [left, left + width, top, top + height],
            optimal_outer_bounds: [
                left - border_size_l.max(margin_l),
                left + width + border_size_r.max(margin_r),
                top - border_size_t.max(margin_t),
                top + height + border_size_b.max(margin_b),
            ],
            content_bounds: None,
            optimal_content_bounds: [
                left + pad_l,
                left + width - pad_r,
                top + pad_t,
                top + height - pad_b,
            ],
            extent: [
                context.extent[0].trunc() as u32,
                context.extent[1].trunc() as u32,
            ],
            scale: context.scale,
        };

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.placement = elapsed;
            });
        }

        // -- Check Visibility ---------------------------------------------------------------- //

        if hidden
            || opacity == 0.0
            || inner_bounds[1] - inner_bounds[0] < 1.0
            || inner_bounds[3] - inner_bounds[2] < 1.0
        {
            bpu.visible = false;

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.visibility = elapsed;
                });
            }

            // NOTE: Eventhough the Bin is hidden, create an entry for each image used in the vertex
            //       data, so that the renderer keeps this image loaded on the gpu.

            let mut vertex_data = ImageMap::new();

            match style.back_image.clone() {
                Some(image_key) => {
                    if image_key.is_vulkano_id() {
                        update_state.back_image_info = None;
                        vertex_data.try_insert(&image_key, Vec::new);
                    } else {
                        let image_info_op = match update_state.back_image_info.as_ref() {
                            Some((last_image_key, image_info)) => {
                                if *last_image_key == image_key {
                                    Some(image_info.clone())
                                } else {
                                    update_state.back_image_info = None;
                                    None
                                }
                            },
                            None => None,
                        }
                        .or_else(|| self.basalt.image_cache_ref().obtain_image_info(&image_key))
                        .or_else(|| {
                            match self.basalt.image_cache_ref().load_from_key(
                                ImageCacheLifetime::Immeditate,
                                (),
                                &image_key,
                            ) {
                                Ok(image_info) => Some(image_info),
                                Err(e) => {
                                    println!(
                                        "[Basalt]: Bin ID: {:?} | Failed to load image: {}",
                                        self.id, e
                                    );
                                    None
                                },
                            }
                        });

                        if let Some(image_info) = image_info_op {
                            if update_state.back_image_info.is_none() {
                                update_state.back_image_info =
                                    Some((image_key.clone(), image_info.clone()));
                            }

                            vertex_data.try_insert(&image_key, Vec::new);
                        }
                    }
                },
                None => {
                    update_state.back_image_info = None;
                },
            }

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.back_image = elapsed;
                });
            }

            // Calculate bounds of custom_verts

            if !style.custom_verts.is_empty() {
                let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];

                for vertex in style.custom_verts.iter() {
                    let x = left + vertex.position.0;
                    let y = top + vertex.position.1;
                    bounds[0] = bounds[0].min(x);
                    bounds[1] = bounds[1].max(x);
                    bounds[2] = bounds[2].min(y);
                    bounds[2] = bounds[2].max(y);
                }

                bpu.content_bounds = Some(bounds);
            }

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.back_vertex = elapsed;
                });
            }

            // Update text for up to date ImageKey's and bounds.

            let content_tlwh = [
                bpu.optimal_content_bounds[2],
                bpu.optimal_content_bounds[0],
                bpu.optimal_content_bounds[1] - bpu.optimal_content_bounds[0],
                bpu.optimal_content_bounds[3] - bpu.optimal_content_bounds[2],
            ];

            update_state
                .text
                .update_buffer(content_tlwh, content_z, opacity, &style, context);

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.text_buffer = elapsed;
                });
            }

            update_state
                .text
                .update_layout(context, self.basalt.image_cache_ref());

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.text_layout = elapsed;
                });
            }

            update_state.text.nonvisible_vertex_data(&mut vertex_data);

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.text_vertex = elapsed;
                });
            }

            if let Some(text_bounds) = update_state.text.bounds() {
                match bpu.content_bounds.as_mut() {
                    Some(content_bounds) => {
                        content_bounds[0] = content_bounds[0].min(text_bounds[0]);
                        content_bounds[1] = content_bounds[1].max(text_bounds[1]);
                        content_bounds[2] = content_bounds[2].min(text_bounds[2]);
                        content_bounds[3] = content_bounds[3].max(text_bounds[3]);
                    },
                    None => {
                        bpu.content_bounds = Some(text_bounds);
                    },
                }
            }

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.overflow = elapsed;
                });
            }

            // Post update things

            let bpu = RwLockWriteGuard::downgrade(bpu);
            self.call_on_update_hooks(&bpu);

            if let Some(metrics_state) = metrics_op.as_mut() {
                metrics_state.segment(|metrics, elapsed| {
                    metrics.post_update = elapsed;
                });
            }

            return (
                vertex_data,
                metrics_op.map(|metrics_state| metrics_state.complete()),
            );
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.visibility = elapsed;
            });
        }

        // -- Background Image --------------------------------------------------------- //

        let (back_image_key, mut back_image_coords) = match style.back_image.clone() {
            Some(image_key) => {
                match image_key.as_vulkano_id() {
                    Some(image_id) => {
                        update_state.back_image_info = None;

                        let image_state =
                            self.basalt.device_resources_ref().image(image_id).unwrap();

                        let [w, h, _] = image_state.image().extent();

                        (image_key, Coords::new(w as f32, h as f32))
                    },
                    None => {
                        let image_info_op = match update_state.back_image_info.as_ref() {
                            Some((last_image_key, image_info)) => {
                                if *last_image_key == image_key {
                                    Some(image_info.clone())
                                } else {
                                    update_state.back_image_info = None;
                                    None
                                }
                            },
                            None => None,
                        }
                        .or_else(|| self.basalt.image_cache_ref().obtain_image_info(&image_key))
                        .or_else(|| {
                            match self.basalt.image_cache_ref().load_from_key(
                                ImageCacheLifetime::Immeditate,
                                (),
                                &image_key,
                            ) {
                                Ok(image_info) => Some(image_info),
                                Err(e) => {
                                    println!(
                                        "[Basalt]: Bin ID: {:?} | Failed to load image: {}",
                                        self.id, e
                                    );
                                    None
                                },
                            }
                        });

                        match image_info_op {
                            Some(image_info) => {
                                if update_state.back_image_info.is_none() {
                                    update_state.back_image_info =
                                        Some((image_key.clone(), image_info.clone()));
                                }

                                (
                                    image_key,
                                    Coords::new(image_info.width as f32, image_info.height as f32),
                                )
                            },
                            None => (ImageKey::NONE, Coords::new(0.0, 0.0)),
                        }
                    },
                }
            },
            None => {
                update_state.back_image_info = None;
                (ImageKey::NONE, Coords::new(0.0, 0.0))
            },
        };

        if let Some(user_coords) = style.back_image_coords.as_ref() {
            back_image_coords.tlwh[0] = user_coords[0];
            back_image_coords.tlwh[1] = user_coords[1];
            back_image_coords.tlwh[2] =
                user_coords[2].clamp(0.0, back_image_coords.tlwh[2] - back_image_coords.tlwh[1]);
            back_image_coords.tlwh[3] =
                user_coords[3].clamp(0.0, back_image_coords.tlwh[3] - back_image_coords.tlwh[0]);
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.back_image = elapsed;
            });
        }

        // -- Borders, Backround & Custom Verts --------------------------------------------- //

        let mut border_color_t = style.border_color_t.unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_b = style.border_color_b.unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_l = style.border_color_l.unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_r = style.border_color_r.unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut back_color = style.back_color.unwrap_or(Color {
            r: 0.0,
            b: 0.0,
            g: 0.0,
            a: 0.0,
        });

        if opacity != 1.0 {
            border_color_t.a *= opacity;
            border_color_b.a *= opacity;
            border_color_l.a *= opacity;
            border_color_r.a *= opacity;
            back_color.a *= opacity;
        }

        let border_radius_tl = style.border_radius_tl.unwrap_or(0.0);
        let border_radius_tr = style.border_radius_tr.unwrap_or(0.0);
        let border_radius_bl = style.border_radius_bl.unwrap_or(0.0);
        let border_radius_br = style.border_radius_br.unwrap_or(0.0);
        let max_radius_t = border_radius_tl.max(border_radius_tr);
        let max_radius_b = border_radius_bl.max(border_radius_br);
        let max_radius_l = border_radius_tl.max(border_radius_bl);
        let max_radius_r = border_radius_tr.max(border_radius_br);
        let mut back_vertexes = Vec::new();

        if back_color.a > 0.0 || !back_image_key.is_none() {
            if max_radius_t > 0.0 {
                let t = top;
                let b = t + max_radius_t;
                let l = left + border_radius_tl;
                let r = left + width - border_radius_tr;

                if l != r {
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, b]);
                }
            }

            if max_radius_b > 0.0 {
                let b = top + height;
                let t = b - max_radius_b;
                let l = left + border_radius_bl;
                let r = left + width - border_radius_br;

                if l != r {
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, b]);
                }
            }

            if max_radius_l > 0.0 {
                let t = top + border_radius_tl;
                let b = (top + height) - border_radius_bl;
                let l = left;
                let r = l + max_radius_l;

                if t != b {
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, b]);
                }
            }

            if max_radius_r > 0.0 {
                let t = top + border_radius_tr;
                let b = (top + height) - border_radius_bl;
                let r = left + width;
                let l = r - max_radius_r;

                if t != b {
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, t]);
                    back_vertexes.push([l, b]);
                    back_vertexes.push([r, b]);
                }
            }

            let t = top + max_radius_t;
            let b = (top + height) - max_radius_b;
            let l = left + max_radius_l;
            let r = (left + width) - max_radius_r;

            if t != b && l != r {
                back_vertexes.push([r, t]);
                back_vertexes.push([l, t]);
                back_vertexes.push([l, b]);
                back_vertexes.push([r, t]);
                back_vertexes.push([l, b]);
                back_vertexes.push([r, b]);
            }
        }

        let mut border_vertexes = Vec::new();

        if border_size_t > 0.0 && border_color_t.a > 0.0 {
            let t = top - border_size_t;
            let b = top;
            let l = left + border_radius_tl;
            let r = left + width - border_radius_tr;
            border_vertexes.push(([r, t], border_color_t));
            border_vertexes.push(([l, t], border_color_t));
            border_vertexes.push(([l, b], border_color_t));
            border_vertexes.push(([r, t], border_color_t));
            border_vertexes.push(([l, b], border_color_t));
            border_vertexes.push(([r, b], border_color_t));
        }

        if border_size_b > 0.0 && border_color_b.a > 0.0 {
            let t = top + height;
            let b = t + border_size_b;
            let l = left + border_radius_bl;
            let r = left + width - border_radius_br;
            border_vertexes.push(([r, t], border_color_b));
            border_vertexes.push(([l, t], border_color_b));
            border_vertexes.push(([l, b], border_color_b));
            border_vertexes.push(([r, t], border_color_b));
            border_vertexes.push(([l, b], border_color_b));
            border_vertexes.push(([r, b], border_color_b));
        }

        if border_size_l > 0.0 && border_color_l.a > 0.0 {
            let t = top + border_radius_tl;
            let b = (top + height) - border_radius_bl;
            let l = left - border_size_l;
            let r = left;
            border_vertexes.push(([r, t], border_color_l));
            border_vertexes.push(([l, t], border_color_l));
            border_vertexes.push(([l, b], border_color_l));
            border_vertexes.push(([r, t], border_color_l));
            border_vertexes.push(([l, b], border_color_l));
            border_vertexes.push(([r, b], border_color_l));
        }

        if border_size_r > 0.0 && border_color_r.a > 0.0 {
            let t = top + border_radius_tr;
            let b = (top + height) - border_radius_br;
            let l = left + width;
            let r = l + border_size_r;
            border_vertexes.push(([r, t], border_color_r));
            border_vertexes.push(([l, t], border_color_r));
            border_vertexes.push(([l, b], border_color_r));
            border_vertexes.push(([r, t], border_color_r));
            border_vertexes.push(([l, b], border_color_r));
            border_vertexes.push(([r, b], border_color_r));
        }

        if border_radius_tl != 0.0 {
            let num_segments: usize = (FRAC_PI_2 * border_radius_tl).ceil() as usize;

            let icp = (0..=num_segments)
                .map(|i| {
                    curve(
                        i as f32 / num_segments as f32,
                        [left, top + border_radius_tl],
                        [left, top],
                        [left + border_radius_tl, top],
                    )
                })
                .collect::<Vec<_>>();

            if back_color.a > 0.0 || !back_image_key.is_none() {
                let cx = left + border_radius_tl;
                let cy = top + border_radius_tl;

                for i in 0..num_segments {
                    back_vertexes.push(icp[i]);
                    back_vertexes.push(icp[i + 1]);
                    back_vertexes.push([cx, cy]);
                }
            }

            if (border_color_t.a > 0.0 && border_size_t > 0.0)
                || (border_color_l.a > 0.0 || border_size_l > 0.0)
            {
                let ocp = (0..=num_segments)
                    .map(|i| {
                        curve(
                            i as f32 / num_segments as f32,
                            [left - border_size_l, top + border_radius_tl],
                            [left - border_size_l, top - border_size_t],
                            [(left) + border_radius_tl, top - border_size_t],
                        )
                    })
                    .collect::<Vec<_>>();

                let colors = (0..=num_segments)
                    .map(|i| {
                        let t = i as f32 / num_segments as f32;

                        Color {
                            r: lerp(t, border_color_l.r, border_color_t.r),
                            g: lerp(t, border_color_l.g, border_color_t.g),
                            b: lerp(t, border_color_l.b, border_color_t.b),
                            a: lerp(t, border_color_l.a, border_color_t.a),
                        }
                    })
                    .collect::<Vec<_>>();

                for i in 0..num_segments {
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i], colors[i]));
                }
            }
        } else if border_size_t > 0.0
            && border_color_t.a > 0.0
            && border_size_l > 0.0
            && border_color_l.a > 0.0
        {
            let t = top - border_size_t;
            let b = top;
            let l = left - border_size_l;
            let r = left;
            border_vertexes.push(([r, t], border_color_t));
            border_vertexes.push(([l, t], border_color_t));
            border_vertexes.push(([r, b], border_color_t));
            border_vertexes.push(([l, t], border_color_l));
            border_vertexes.push(([l, b], border_color_l));
            border_vertexes.push(([r, b], border_color_l));
        }

        if border_radius_tr != 0.0 {
            let num_segments: usize = (FRAC_PI_2 * border_radius_tr).ceil() as usize;

            let icp = (0..=num_segments)
                .map(|i| {
                    curve(
                        i as f32 / num_segments as f32,
                        [left + width, top + border_radius_tr],
                        [left + width, top],
                        [left + width - border_radius_tr, top],
                    )
                })
                .collect::<Vec<_>>();

            if back_color.a > 0.0 || !back_image_key.is_none() {
                let cx = left + width - border_radius_tr;
                let cy = top + border_radius_tr;

                for i in 0..num_segments {
                    back_vertexes.push(icp[i]);
                    back_vertexes.push(icp[i + 1]);
                    back_vertexes.push([cx, cy]);
                }
            }

            if (border_color_t.a > 0.0 && border_size_t > 0.0)
                || (border_color_r.a > 0.0 || border_size_r > 0.0)
            {
                let ocp = (0..=num_segments)
                    .map(|i| {
                        curve(
                            i as f32 / num_segments as f32,
                            [left + width + border_size_r, top + border_radius_tr],
                            [left + width + border_size_r, top - border_size_t],
                            [left + width - border_radius_tr, top - border_size_t],
                        )
                    })
                    .collect::<Vec<_>>();

                let colors = (0..=num_segments)
                    .map(|i| {
                        let t = i as f32 / num_segments as f32;

                        Color {
                            r: lerp(t, border_color_r.r, border_color_t.r),
                            g: lerp(t, border_color_r.g, border_color_t.g),
                            b: lerp(t, border_color_r.b, border_color_t.b),
                            a: lerp(t, border_color_r.a, border_color_t.a),
                        }
                    })
                    .collect::<Vec<_>>();

                for i in 0..num_segments {
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i], colors[i]));
                }
            }
        } else if border_size_t > 0.0
            && border_color_t.a > 0.0
            && border_size_r > 0.0
            && border_color_r.a > 0.0
        {
            let t = top - border_size_t;
            let b = top;
            let l = left + width;
            let r = left + width + border_size_r;

            border_vertexes.push(([r, t], border_color_t));
            border_vertexes.push(([l, t], border_color_t));
            border_vertexes.push(([l, b], border_color_t));
            border_vertexes.push(([r, t], border_color_r));
            border_vertexes.push(([l, b], border_color_r));
            border_vertexes.push(([r, b], border_color_r));
        }

        if border_radius_bl != 0.0 {
            let num_segments: usize = (FRAC_PI_2 * border_radius_bl).ceil() as usize;

            let icp = (0..=num_segments)
                .map(|i| {
                    curve(
                        i as f32 / num_segments as f32,
                        [left, top + height - border_radius_bl],
                        [left, top + height],
                        [left + border_radius_bl, top + height],
                    )
                })
                .collect::<Vec<_>>();

            if back_color.a > 0.0 || !back_image_key.is_none() {
                let cx = left + border_radius_bl;
                let cy = top + height - border_radius_bl;

                for i in 0..num_segments {
                    back_vertexes.push([cx, cy]);
                    back_vertexes.push(icp[i + 1]);
                    back_vertexes.push(icp[i]);
                }
            }

            if (border_color_b.a > 0.0 && border_size_b > 0.0)
                || (border_color_l.a > 0.0 || border_size_l > 0.0)
            {
                let ocp = (0..=num_segments)
                    .map(|i| {
                        curve(
                            i as f32 / num_segments as f32,
                            [left - border_size_l, top + height - border_radius_bl],
                            [left - border_size_l, top + height + border_size_b],
                            [left + border_radius_bl, top + height + border_size_b],
                        )
                    })
                    .collect::<Vec<_>>();

                let colors = (0..=num_segments)
                    .map(|i| {
                        let t = i as f32 / num_segments as f32;

                        Color {
                            r: lerp(t, border_color_l.r, border_color_b.r),
                            g: lerp(t, border_color_l.g, border_color_b.g),
                            b: lerp(t, border_color_l.b, border_color_b.b),
                            a: lerp(t, border_color_l.a, border_color_b.a),
                        }
                    })
                    .collect::<Vec<_>>();

                for i in 0..num_segments {
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i], colors[i]));
                }
            }
        } else if border_size_b > 0.0
            && border_color_b.a > 0.0
            && border_size_l > 0.0
            && border_color_l.a > 0.0
        {
            let t = top + height;
            let b = t + border_size_b;
            let l = left - border_size_l;
            let r = left;
            border_vertexes.push(([r, t], border_color_b));
            border_vertexes.push(([l, b], border_color_b));
            border_vertexes.push(([r, b], border_color_b));
            border_vertexes.push(([r, t], border_color_l));
            border_vertexes.push(([l, t], border_color_l));
            border_vertexes.push(([l, b], border_color_l));
        }

        if border_radius_br != 0.0 {
            let num_segments: usize = (FRAC_PI_2 * border_radius_br).ceil() as usize;

            let icp = (0..=num_segments)
                .map(|i| {
                    curve(
                        i as f32 / num_segments as f32,
                        [left + width, top + height - border_radius_br],
                        [left + width, top + height],
                        [left + width - border_radius_br, top + height],
                    )
                })
                .collect::<Vec<_>>();

            if back_color.a > 0.0 || !back_image_key.is_none() {
                let cx = left + width - border_radius_br;
                let cy = top + height - border_radius_br;

                for i in 0..num_segments {
                    back_vertexes.push([cx, cy]);
                    back_vertexes.push(icp[i + 1]);
                    back_vertexes.push(icp[i]);
                }
            }

            if (border_color_b.a > 0.0 && border_size_b > 0.0)
                || (border_color_r.a > 0.0 || border_size_r > 0.0)
            {
                let ocp = (0..=num_segments)
                    .map(|i| {
                        curve(
                            i as f32 / num_segments as f32,
                            [
                                left + width + border_size_r,
                                top + height - border_radius_br,
                            ],
                            [left + width + border_size_r, top + height + border_size_b],
                            [
                                left + width - border_radius_br,
                                top + height + border_size_b,
                            ],
                        )
                    })
                    .collect::<Vec<_>>();

                let colors = (0..=num_segments)
                    .map(|i| {
                        let t = i as f32 / num_segments as f32;

                        Color {
                            r: lerp(t, border_color_r.r, border_color_b.r),
                            g: lerp(t, border_color_r.g, border_color_b.g),
                            b: lerp(t, border_color_r.b, border_color_b.b),
                            a: lerp(t, border_color_r.a, border_color_b.a),
                        }
                    })
                    .collect::<Vec<_>>();

                for i in 0..num_segments {
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i + 1], colors[i + 1]));
                    border_vertexes.push((ocp[i], colors[i]));
                    border_vertexes.push((icp[i], colors[i]));
                }
            }
        } else if border_size_b > 0.0
            && border_color_b.a > 0.0
            && border_size_r > 0.0
            && border_color_r.a > 0.0
        {
            let t = top + height;
            let b = t + border_size_b;
            let l = left + width;
            let r = l + border_size_r;
            border_vertexes.push(([l, t], border_color_b));
            border_vertexes.push(([l, b], border_color_b));
            border_vertexes.push(([r, b], border_color_b));
            border_vertexes.push(([r, t], border_color_r));
            border_vertexes.push(([l, t], border_color_r));
            border_vertexes.push(([r, b], border_color_r));
        }

        let mut outer_vert_data: ImageMap<Vec<ItfVertInfo>> = ImageMap::new();

        if !back_image_key.is_none() {
            let ty = style
                .back_image_effect
                .as_ref()
                .map(|effect| effect.vert_type())
                .unwrap_or(100);
            let color = back_color.rgbaf_array();

            outer_vert_data.modify(&back_image_key, Vec::new, |vertexes| {
                vertexes.extend(back_vertexes.into_iter().map(|[x, y]| {
                    ItfVertInfo {
                        position: [x, y, base_z],
                        coords: [
                            back_image_coords.x_pct((x - left) / width),
                            back_image_coords.y_pct((y - top) / height),
                        ],
                        color,
                        ty,
                        tex_i: 0,
                    }
                }));
            });
        } else {
            let color = back_color.rgbaf_array();

            outer_vert_data.modify(&ImageKey::NONE, Vec::new, |vertexes| {
                vertexes.extend(back_vertexes.into_iter().map(|[x, y]| {
                    ItfVertInfo {
                        position: [x, y, base_z],
                        coords: [0.0; 2],
                        color,
                        ty: 0,
                        tex_i: 0,
                    }
                }));
            });
        }

        if !border_vertexes.is_empty() {
            outer_vert_data.modify(&ImageKey::NONE, Vec::new, |vertexes| {
                vertexes.extend(border_vertexes.into_iter().map(|([x, y], color)| {
                    ItfVertInfo {
                        position: [x, y, base_z],
                        coords: [0.0; 2],
                        color: color.rgbaf_array(),
                        ty: 0,
                        tex_i: 0,
                    }
                }));
            });
        }

        let mut inner_vert_data: ImageMap<Vec<ItfVertInfo>> = ImageMap::new();

        if !style.custom_verts.is_empty() {
            let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];

            inner_vert_data.insert(
                ImageKey::NONE,
                style
                    .custom_verts
                    .iter()
                    .map(|vertex| {
                        let z = if vertex.position.2 == 0 {
                            content_z
                        } else {
                            z_unorm(vertex.position.2)
                        };

                        let x = left + vertex.position.0;
                        let y = top + vertex.position.1;
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].max(x);
                        bounds[2] = bounds[2].min(y);
                        bounds[2] = bounds[2].max(y);
                        let mut color = vertex.color;
                        color.a *= opacity;

                        ItfVertInfo {
                            position: [x, y, z],
                            coords: [0.0, 0.0],
                            color: color.rgbaf_array(),
                            ty: 0,
                            tex_i: 0,
                        }
                    })
                    .collect(),
            );

            bpu.content_bounds = Some(bounds);
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.back_vertex = elapsed;
            });
        }

        // -- Text -------------------------------------------------------------------------- //

        let content_tlwh = [
            bpu.optimal_content_bounds[2],
            bpu.optimal_content_bounds[0],
            bpu.optimal_content_bounds[1] - bpu.optimal_content_bounds[0],
            bpu.optimal_content_bounds[3] - bpu.optimal_content_bounds[2],
        ];

        update_state
            .text
            .update_buffer(content_tlwh, content_z, opacity, &style, context);

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.text_buffer = elapsed;
            });
        }

        update_state
            .text
            .update_layout(context, self.basalt.image_cache_ref());

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.text_layout = elapsed;
            });
        }

        update_state
            .text
            .update_vertexes(Some(&mut inner_vert_data));

        if let Some(text_bounds) = update_state.text.bounds() {
            match bpu.content_bounds.as_mut() {
                Some(content_bounds) => {
                    content_bounds[0] = content_bounds[0].min(text_bounds[0]);
                    content_bounds[1] = content_bounds[1].max(text_bounds[1]);
                    content_bounds[2] = content_bounds[2].min(text_bounds[2]);
                    content_bounds[3] = content_bounds[3].max(text_bounds[3]);
                },
                None => {
                    bpu.content_bounds = Some(text_bounds);
                },
            }
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.text_vertex = elapsed;
            });
        }

        // -- Bounds Checks --------------------------------------------------------------------- //

        for (bounds, vert_data) in [
            (inner_bounds, inner_vert_data.values_mut()),
            (outer_bounds, outer_vert_data.values_mut()),
        ] {
            for vertexes in vert_data {
                let mut remove_indexes = Vec::new();
                let mut x_lt = Vec::with_capacity(2);
                let mut x_gt = Vec::with_capacity(2);
                let mut y_lt = Vec::with_capacity(2);
                let mut y_gt = Vec::with_capacity(2);

                for t in 0..(vertexes.len() / 3) {
                    let v = t * 3;
                    let ax_lt = vertexes[v].position[0] < bounds[0];
                    let bx_lt = vertexes[v + 1].position[0] < bounds[0];
                    let cx_lt = vertexes[v + 2].position[0] < bounds[0];
                    let ax_gt = vertexes[v].position[0] > bounds[1];
                    let bx_gt = vertexes[v + 1].position[0] > bounds[1];
                    let cx_gt = vertexes[v + 2].position[0] > bounds[1];
                    let ay_lt = vertexes[v].position[1] < bounds[2];
                    let by_lt = vertexes[v + 1].position[1] < bounds[2];
                    let cy_lt = vertexes[v + 2].position[1] < bounds[2];
                    let ay_gt = vertexes[v].position[1] > bounds[3];
                    let by_gt = vertexes[v + 1].position[1] > bounds[3];
                    let cy_gt = vertexes[v + 2].position[1] > bounds[3];

                    if !ax_lt
                        && !bx_lt
                        && !cx_lt
                        && !ax_gt
                        && !bx_gt
                        && !cx_gt
                        && !ay_lt
                        && !by_lt
                        && !cy_lt
                        && !ay_gt
                        && !by_gt
                        && !cy_gt
                    {
                        continue;
                    }

                    if (ax_lt && bx_lt && cx_lt)
                        || (ax_gt && bx_gt && cx_gt)
                        || (ay_lt && by_lt && cy_lt)
                        || (ay_gt && by_gt && cy_gt)
                    {
                        remove_indexes.push(v);
                        remove_indexes.push(v + 1);
                        remove_indexes.push(v + 2);
                        continue;
                    }

                    // TODO: this is an approximation

                    let p_dim = [
                        (vertexes[v].position[1]
                            .max(vertexes[v + 1].position[1].max(vertexes[v + 2].position[1]))
                            - vertexes[v].position[1]
                                .min(vertexes[v + 1].position[1].min(vertexes[v + 2].position[1]))),
                        (vertexes[v].position[0]
                            .max(vertexes[v + 1].position[0].max(vertexes[v + 2].position[0]))
                            - vertexes[v].position[0]
                                .min(vertexes[v + 1].position[0].min(vertexes[v + 2].position[0]))),
                    ];

                    let c_dim = [
                        (vertexes[v].coords[1]
                            .max(vertexes[v + 1].coords[1].max(vertexes[v + 2].coords[1]))
                            - vertexes[v].coords[1]
                                .min(vertexes[v + 1].coords[1].min(vertexes[v + 2].coords[1]))),
                        (vertexes[v].coords[0]
                            .max(vertexes[v + 1].coords[0].max(vertexes[v + 2].coords[0]))
                            - vertexes[v].coords[0]
                                .min(vertexes[v + 1].coords[0].min(vertexes[v + 2].coords[0]))),
                    ];

                    if ax_lt {
                        x_lt.push(v);
                    }
                    if bx_lt {
                        x_lt.push(v + 1);
                    }
                    if cx_lt {
                        x_lt.push(v + 2);
                    }
                    if ax_gt {
                        x_gt.push(v);
                    }
                    if bx_gt {
                        x_gt.push(v + 1);
                    }
                    if cx_gt {
                        x_gt.push(v + 2);
                    }
                    if ay_lt {
                        y_lt.push(v);
                    }
                    if by_lt {
                        y_lt.push(v + 1);
                    }
                    if cy_lt {
                        y_lt.push(v + 1);
                    }
                    if ay_gt {
                        y_gt.push(v);
                    }
                    if by_gt {
                        y_gt.push(v + 1);
                    }
                    if cy_gt {
                        y_gt.push(v + 2);
                    }

                    for i in x_lt.drain(..) {
                        vertexes[i].coords[0] +=
                            c_dim[0] * ((bounds[0] - vertexes[i].position[0]) / p_dim[0]);
                        vertexes[i].position[0] = bounds[0];
                    }

                    for i in x_gt.drain(..) {
                        vertexes[i].coords[0] -=
                            c_dim[0] * ((vertexes[i].position[0] - bounds[1]) / p_dim[0]);
                        vertexes[i].position[0] = bounds[1];
                    }

                    for i in y_lt.drain(..) {
                        vertexes[i].coords[1] +=
                            c_dim[1] * ((bounds[2] - vertexes[i].position[1]) / p_dim[1]);
                        vertexes[i].position[1] = bounds[2];
                    }

                    for i in y_gt.drain(..) {
                        vertexes[i].coords[1] -=
                            c_dim[1] * ((vertexes[i].position[1] - bounds[3]) / p_dim[1]);
                        vertexes[i].position[1] = bounds[3];
                    }
                }

                for i in remove_indexes.into_iter().rev() {
                    vertexes.remove(i);
                }
            }
        }

        let mut vert_data = inner_vert_data;

        for (image_key, mut vertexes) in outer_vert_data {
            vert_data.modify(&image_key, Vec::new, |v| v.append(&mut vertexes));
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.overflow = elapsed;
            });
        }

        // ----------------------------------------------------------------------------- //

        for verts in vert_data.values_mut() {
            scale_verts(&context.extent, context.scale, verts);
            verts.shrink_to_fit();
        }

        if let Some(metrics_state) = metrics_op.as_mut() {
            metrics_state.segment(|metrics, elapsed| {
                metrics.vertex_scale = elapsed;
            });
        }

        let bpu = RwLockWriteGuard::downgrade(bpu);
        self.call_on_update_hooks(&bpu);

        (
            vert_data,
            metrics_op.map(|metrics_state| metrics_state.complete()),
        )
    }
}

#[inline(always)]
fn z_unorm(z: i16) -> f32 {
    (z as f32 + i16::MAX as f32) / u16::MAX as f32
}

#[inline(always)]
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    (t * b) + ((1.0 - t) * a)
}

#[inline(always)]
fn curve(t: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> [f32; 2] {
    [
        lerp(t, lerp(t, a[0], b[0]), lerp(t, b[0], c[0])),
        lerp(t, lerp(t, a[1], b[1]), lerp(t, b[1], c[1])),
    ]
}
