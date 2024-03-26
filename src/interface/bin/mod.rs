pub mod style;

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::ops::{AddAssign, DivAssign};
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Barrier, Weak};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapAny;
use cosmic_text as text;
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};

use crate::image_cache::{ImageCacheKey, ImageCacheLifetime, ImageData, ImageFormat};
use crate::input::{
    Char, InputHookCtrl, InputHookID, InputHookTarget, KeyCombo, LocalCursorState, LocalKeyState,
    MouseButton, WindowState,
};
use crate::interface::{
    scale_verts, BinPosition, BinStyle, BinStyleValidation, Color, FontStretch, FontStyle,
    FontWeight, ItfVertInfo, TextHoriAlign, TextVertAlign, TextWrap,
};
use crate::interval::IntvlHookCtrl;
use crate::render::{ImageSource, UpdateContext};
use crate::window::Window;
use crate::Basalt;

/// ID of a `Bin`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BinID(pub(super) u64);

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
    /// Target Extent (Generally Window Size)
    pub extent: [u32; 2],
    /// UI Scale Used
    pub scale: f32,
    text_state: Option<TextState>,
}

#[derive(Clone)]
pub(crate) struct BinPlacement {
    z: i16,
    tlwh: [f32; 4],
    bounds: [f32; 4],
    opacity: f32,
    hidden: bool,
}

#[derive(Default)]
struct BinHrchy {
    parent: Option<Weak<Bin>>,
    children: Vec<Weak<Bin>>,
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TextState {
    text: String,
    style: TextStyle,
    body_from_t: f32,
    body_from_l: f32,
    vertex_data: HashMap<ImageCacheKey, Vec<ItfVertInfo>>,
}

#[derive(PartialEq, Debug, Clone)]
struct TextStyle {
    text_height: f32,
    line_height: f32,
    body_width: f32,
    body_height: f32,
    wrap: TextWrap,
    vert_align: TextVertAlign,
    hori_align: TextHoriAlign,
    font_family: Option<String>,
    font_weight: Option<FontWeight>,
    font_stretch: Option<FontStretch>,
    font_style: Option<FontStyle>,
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

    fn tlwh(&self) -> [f32; 4] {
        self.tlwh
    }

    fn top_left(&self) -> [f32; 2] {
        [self.tlwh[0], self.tlwh[1]]
    }

    fn top_right(&self) -> [f32; 2] {
        [self.tlwh[0] + self.tlwh[2], self.tlwh[1]]
    }

    fn bottom_left(&self) -> [f32; 2] {
        [self.tlwh[0], self.tlwh[1] + self.tlwh[3]]
    }

    fn bottom_right(&self) -> [f32; 2] {
        [self.tlwh[0] + self.tlwh[2], self.tlwh[1] + self.tlwh[3]]
    }

    fn width_height(&self) -> [f32; 2] {
        [self.tlwh[2], self.tlwh[3]]
    }
}

struct GlyphImageAssociatedData {
    vertex_type: i32,
    placement_top: i32,
    placement_left: i32,
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
    pub text: f32,
    pub overflow: f32,
    pub vertex_scale: f32,
    pub post_update: f32,
}

impl AddAssign for OVDPerfMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.total += rhs.total;
        self.style += rhs.style;
        self.placement += rhs.placement;
        self.visibility += rhs.visibility;
        self.back_image += rhs.back_image;
        self.back_vertex += rhs.back_vertex;
        self.text += rhs.text;
        self.overflow += rhs.overflow;
        self.vertex_scale += rhs.vertex_scale;
        self.post_update += rhs.post_update;
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
        self.text /= rhs;
        self.overflow /= rhs;
        self.vertex_scale /= rhs;
        self.post_update /= rhs;
    }
}

/// Fundamental UI component.
pub struct Bin {
    basalt: Arc<Basalt>,
    id: BinID,
    associated_window: Mutex<Option<Weak<Window>>>,
    hrchy: ArcSwapAny<Arc<BinHrchy>>,
    style: ArcSwapAny<Arc<BinStyle>>,
    initial: AtomicBool,
    post_update: RwLock<BinPostUpdate>,
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

        let this_hrchy = self.hrchy.load_full();

        if let Some(parent) = this_hrchy
            .parent
            .as_ref()
            .and_then(|parent| parent.upgrade())
        {
            let parent_hrchy = parent.hrchy.load_full();
            let mut children_removed = Vec::new();

            let children = parent_hrchy
                .children
                .iter()
                .filter_map(|child_wk| {
                    if child_wk.upgrade().is_some() {
                        Some(child_wk.clone())
                    } else {
                        children_removed.push(child_wk.clone());
                        None
                    }
                })
                .collect();

            if !children_removed.is_empty() {
                parent.hrchy.store(Arc::new(BinHrchy {
                    children,
                    parent: parent_hrchy.parent.clone(),
                }));

                parent.call_children_removed_hooks(children_removed);
            }
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
            hrchy: ArcSwapAny::from(Arc::new(BinHrchy::default())),
            style: ArcSwapAny::new(Arc::new(BinStyle::default())),
            initial: AtomicBool::new(true),
            post_update: RwLock::new(BinPostUpdate::default()),
            input_hook_ids: Mutex::new(Vec::new()),
            keep_alive_objects: Mutex::new(Vec::new()),
            internal_hooks: Mutex::new(HashMap::from([
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
            .load_full()
            .parent
            .as_ref()
            .and_then(|v| v.upgrade())
    }

    /// Return the ancestors of this `Bin` where the order is from parent, parent's
    /// parent, parent's parent's parent, etc...
    pub fn ancestors(&self) -> Vec<Arc<Bin>> {
        let mut out = Vec::new();
        let mut check_wk_op = self.hrchy.load_full().parent.clone();

        while let Some(check_wk) = check_wk_op.take() {
            if let Some(check) = check_wk.upgrade() {
                out.push(check.clone());
                check_wk_op = check.hrchy.load_full().parent.clone();
            }
        }

        out
    }

    /// Return the children of this `Bin`
    pub fn children(&self) -> Vec<Arc<Bin>> {
        self.hrchy
            .load_full()
            .children
            .iter()
            .filter_map(|wk| wk.upgrade())
            .collect()
    }

    /// Return the children of this `Bin` recursively.
    ///
    /// ***Note:** There is no order to the result.*
    pub fn children_recursive(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let mut out = Vec::new();
        let mut to_check = vec![self.clone()];

        while let Some(child) = to_check.pop() {
            to_check.append(&mut child.children());
            out.push(child);
        }

        out
    }

    /// Return the children of this `Bin` recursively including itself.
    ///
    /// ***Note:** There is no order to the result.*
    pub fn children_recursive_with_self(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let mut out = vec![self.clone()];
        let mut to_check = vec![self.clone()];

        while let Some(child) = to_check.pop() {
            to_check.append(&mut child.children());
            out.push(child);
        }

        out
    }

    /// Add a child to this `Bin`.
    pub fn add_child(self: &Arc<Self>, child: Arc<Bin>) {
        let child_hrchy = child.hrchy.load_full();

        child.hrchy.store(Arc::new(BinHrchy {
            parent: Some(Arc::downgrade(self)),
            children: child_hrchy.children.clone(),
        }));

        let this_hrchy = self.hrchy.load_full();
        let mut children = this_hrchy.children.clone();
        children.push(Arc::downgrade(&child));

        self.hrchy.store(Arc::new(BinHrchy {
            children,
            parent: this_hrchy.parent.clone(),
        }));

        self.call_children_added_hooks(vec![child]);
    }

    /// Add multiple children to this `Bin`.
    pub fn add_children(self: &Arc<Self>, children: Vec<Arc<Bin>>) {
        let this_hrchy = self.hrchy.load_full();
        let mut this_children = this_hrchy.children.clone();

        for child in children.iter() {
            this_children.push(Arc::downgrade(child));
            let child_hrchy = child.hrchy.load_full();

            child.hrchy.store(Arc::new(BinHrchy {
                parent: Some(Arc::downgrade(self)),
                children: child_hrchy.children.clone(),
            }));
        }

        self.hrchy.store(Arc::new(BinHrchy {
            children: this_children,
            parent: this_hrchy.parent.clone(),
        }));

        self.call_children_added_hooks(children);
    }

    /// Take the children from this `Bin`.
    pub fn take_children(self: &Arc<Self>) -> Vec<Arc<Bin>> {
        let this_hrchy = self.hrchy.load_full();
        let mut children = Vec::new();

        for child in this_hrchy.children.iter() {
            if let Some(child) = child.upgrade() {
                let child_hrchy = child.hrchy.load_full();

                child.hrchy.store(Arc::new(BinHrchy {
                    parent: None,
                    children: child_hrchy.children.clone(),
                }));

                children.push(child);
            }
        }

        self.hrchy.store(Arc::new(BinHrchy {
            children: Vec::new(),
            parent: this_hrchy.parent.clone(),
        }));

        self.call_children_removed_hooks(this_hrchy.children.clone());
        children
    }

    /// Obtain an `Arc` of `BinStyle` of this `Bin`.
    ///
    /// This is useful where it is only needed to inspect the style of the `Bin`.
    pub fn style(&self) -> Arc<BinStyle> {
        self.style.load().clone()
    }

    /// Obtain a copy of `BinStyle`  of this `Bin`.
    pub fn style_copy(&self) -> BinStyle {
        self.style.load().as_ref().clone()
    }

    /// Update the style of this `Bin`.
    #[track_caller]
    pub fn style_update(self: &Arc<Self>, copy: BinStyle) -> BinStyleValidation {
        let validation = copy.validate(self);

        if !validation.errors_present() {
            self.style.store(Arc::new(copy));
            self.initial.store(false, atomic::Ordering::SeqCst);
            self.trigger_recursive_update();
        }

        validation
    }

    /// Check if this `Bin` is hidden.
    ///
    /// ***Note:** This is based on the `BinStyle.hidden` value, not if it is offscreen.*
    pub fn is_hidden(&self) -> bool {
        self.is_hidden_inner(None)
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

    fn is_hidden_inner(&self, style_: Option<&BinStyle>) -> bool {
        match match style_ {
            Some(style) => style.hidden.unwrap_or(false),
            None => self.style().hidden.unwrap_or(false),
        } {
            true => true,
            false => {
                match self.parent() {
                    Some(parent) => parent.is_hidden_inner(None),
                    None => false,
                }
            },
        }
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
        let style = self.style();
        let mut overflow_t: f32 = 0.0;
        let mut overflow_b: f32 = 0.0;

        for child in self.children() {
            let child_bpu = child.post_update.read();

            if child_bpu.floating {
                overflow_t = overflow_t.max(
                    (self_bpu.optimal_inner_bounds[2] + style.pad_t.unwrap_or(0.0))
                        - child_bpu.optimal_outer_bounds[2],
                );
                overflow_b = overflow_b.max(
                    child_bpu.optimal_outer_bounds[3]
                        - (self_bpu.optimal_inner_bounds[3] - style.pad_b.unwrap_or(0.0)),
                );
            } else {
                overflow_t = overflow_t
                    .max(self_bpu.optimal_inner_bounds[2] - child_bpu.optimal_outer_bounds[2]);
                overflow_b = overflow_b
                    .max(child_bpu.optimal_outer_bounds[3] - self_bpu.optimal_inner_bounds[3]);
            }
        }

        overflow_t + overflow_b
    }

    /// Calculate the amount of horizontal overflow.
    pub fn calc_hori_overflow(self: &Arc<Bin>) -> f32 {
        let self_bpu = self.post_update.read();
        let style = self.style();
        let mut overflow_l: f32 = 0.0;
        let mut overflow_r: f32 = 0.0;

        for child in self.children() {
            let child_bpu = child.post_update.read();

            if child_bpu.floating {
                overflow_l = overflow_l.max(
                    (self_bpu.optimal_inner_bounds[0] + style.pad_l.unwrap_or(0.0))
                        - child_bpu.optimal_outer_bounds[0],
                );
                overflow_r = overflow_r.max(
                    child_bpu.optimal_outer_bounds[1]
                        - (self_bpu.optimal_inner_bounds[1] - style.pad_r.unwrap_or(0.0)),
                );
            } else {
                overflow_l = overflow_l
                    .max(self_bpu.optimal_inner_bounds[0] - child_bpu.optimal_outer_bounds[0]);
                overflow_r = overflow_r
                    .max(child_bpu.optimal_outer_bounds[1] - self_bpu.optimal_inner_bounds[1]);
            }
        }

        overflow_l + overflow_r
    }

    /// Check if the mouse is inside of this `Bin`.
    ///
    /// ***Note:** This does not check the window.*
    pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
        if self.is_hidden() {
            return false;
        }

        let post = self.post_update.read();

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
            return placement.clone();
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
        let extent = context.extent;
        let position = style.position.unwrap_or(BinPosition::Window);

        if position == BinPosition::Floating {
            let parent = self.parent().unwrap();
            let parent_plmt = parent.calc_placement(context);

            let (padding_tblr, scroll_xy) = {
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
                    let sibling_style = if sibling.id == self.id {
                        style.clone()
                    } else {
                        sibling.style()
                    };

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
                        weight: i as i16, // TODO: Configurable
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

            // TODO: This is row based, add col based?

            let mut x = 0.0;
            let mut y = 0.0;
            let mut row_height = 0.0;
            let mut row_bins = 0;

            for sibling in siblings {
                if sibling.this {
                    let effective_width =
                        sibling.size_xy[0] + sibling.margin_tblr[2] + sibling.margin_tblr[3];

                    if x + effective_width > body_width && row_bins != 0 {
                        x = 0.0;
                        y += row_height;
                    }

                    let top = parent_plmt.tlwh[0] + y + padding_tblr[0] + sibling.margin_tblr[0]
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

                    return BinPlacement {
                        z,
                        tlwh: [top, left, width, height],
                        bounds: [x_bounds[0], x_bounds[1], y_bounds[0], y_bounds[1]],
                        opacity,
                        hidden,
                    };
                } else {
                    let effective_width =
                        sibling.size_xy[0] + sibling.margin_tblr[2] + sibling.margin_tblr[3];
                    let effective_height =
                        sibling.size_xy[1] + sibling.margin_tblr[0] + sibling.margin_tblr[1];

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
                        let parent_style = parent.style();
                        (
                            parent.calc_placement(context),
                            [
                                parent_style.scroll_x.unwrap_or(0.0),
                                parent_style.scroll_y.unwrap_or(0.0),
                            ],
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
                match style.pos_from_t_pct {
                    Some(top_pct) => Some((top_pct / 100.0) * parent_plmt.tlwh[3]),
                    None => None,
                }
            },
        }
        .map(|top| top + style.pos_from_t_offset.unwrap_or(0.0));

        let bottom_op = match style.pos_from_b {
            Some(bottom) => Some(bottom),
            None => {
                match style.pos_from_b_pct {
                    Some(bottom_pct) => Some((bottom_pct / 100.0) * parent_plmt.tlwh[3]),
                    None => None,
                }
            },
        }
        .map(|bottom| bottom + style.pos_from_b_offset.unwrap_or(0.0));

        let left_op = match style.pos_from_l {
            Some(left) => Some(left),
            None => {
                match style.pos_from_l_pct {
                    Some(left_pct) => Some((left_pct / 100.0) * parent_plmt.tlwh[2]),
                    None => None,
                }
            },
        }
        .map(|left| left + style.pos_from_l_offset.unwrap_or(0.0));

        let right_op = match style.pos_from_r {
            Some(right) => Some(right),
            None => {
                match style.pos_from_r_pct {
                    Some(right_pct) => Some((right_pct / 100.0) * parent_plmt.tlwh[2]),
                    None => None,
                }
            },
        }
        .map(|right| right + style.pos_from_r_offset.unwrap_or(0.0));

        let width_op = match style.width {
            Some(width) => Some(width),
            None => {
                match style.width_pct {
                    Some(width_pct) => Some((width_pct / 100.0) * parent_plmt.tlwh[2]),
                    None => None,
                }
            },
        }
        .map(|width| width + style.width_offset.unwrap_or(0.0));

        let height_op = match style.height {
            Some(height) => Some(height),
            None => {
                match style.height_pct {
                    Some(height_pct) => Some((height_pct / 100.0) * parent_plmt.tlwh[3]),
                    None => None,
                }
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
    ) -> (
        HashMap<ImageSource, Vec<ItfVertInfo>>,
        Option<OVDPerfMetrics>,
    ) {
        let mut metrics_op = if context.metrics_enabled {
            let inst = Instant::now();
            Some((inst, inst, OVDPerfMetrics::default()))
        } else {
            None
        };

        // -- Update Check ------------------------------------------------------------------ //

        if self.initial.load(atomic::Ordering::SeqCst) {
            return (HashMap::new(), None);
        }

        // -- Obtain BinPostUpdate & Style --------------------------------------------------- //

        let mut bpu = self.post_update.write();
        let style = self.style();

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.style = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
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

        let last_text_state = bpu.text_state.take();
        let [top, left, width, height] = tlwh;
        let border_size_t = style.border_size_t.unwrap_or(0.0);
        let border_size_b = style.border_size_b.unwrap_or(0.0);
        let border_size_l = style.border_size_l.unwrap_or(0.0);
        let border_size_r = style.border_size_r.unwrap_or(0.0);
        let margin_t = style.margin_t.unwrap_or(0.0);
        let margin_b = style.margin_b.unwrap_or(0.0);
        let margin_l = style.margin_l.unwrap_or(0.0);
        let margin_r = style.margin_r.unwrap_or(0.0);

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
            text_state: last_text_state,
            extent: [
                context.extent[0].trunc() as u32,
                context.extent[1].trunc() as u32,
            ],
            scale: context.scale,
        };

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.placement = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // -- Check Visibility ---------------------------------------------------------------- //

        if hidden
            || opacity == 0.0
            || inner_bounds[1] - inner_bounds[0] < 1.0
            || inner_bounds[3] - inner_bounds[2] < 1.0
        {
            // NOTE: Eventhough the Bin is hidden, create an entry for each image used in the vertex
            //       data, so that the renderer keeps this image loaded on the gpu.

            let mut vertex_data = HashMap::new();

            match style.back_image.clone() {
                Some(image_cache_key) => {
                    if self
                        .basalt
                        .image_cache_ref()
                        .obtain_image_info(image_cache_key.clone())
                        .is_some()
                    {
                        vertex_data
                            .entry(ImageSource::Cache(image_cache_key))
                            .or_default();
                    }
                },
                None => {
                    if let Some(image_vk) = style.back_image_vk.clone() {
                        vertex_data
                            .entry(ImageSource::Vulkano(image_vk))
                            .or_default();
                    }
                },
            }

            if let Some(text_state) = bpu.text_state.as_ref() {
                for image_cache_key in text_state.vertex_data.keys() {
                    vertex_data
                        .entry(ImageSource::Cache(image_cache_key.clone()))
                        .or_default();
                }
            }

            bpu.visible = false;
            let bpu = RwLockWriteGuard::downgrade(bpu);
            self.call_on_update_hooks(&bpu);

            let metrics_op = metrics_op.take().map(|(inst, inst_total, mut metrics)| {
                metrics.visibility = inst.elapsed().as_micros() as f32 / 1000.0;
                metrics.total = inst_total.elapsed().as_micros() as f32 / 1000.0;
                metrics
            });

            return (vertex_data, metrics_op);
        }

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.visibility = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // -- Background Image --------------------------------------------------------- //

        let (back_image_src, mut back_image_coords) = match style.back_image.clone() {
            Some(image_cache_key) => {
                match self
                    .basalt
                    .image_cache_ref()
                    .obtain_image_info(image_cache_key.clone())
                {
                    Some(image_info) => {
                        (
                            ImageSource::Cache(image_cache_key),
                            Coords::new(image_info.width as f32, image_info.height as f32),
                        )
                    },
                    None => {
                        match &image_cache_key {
                            ImageCacheKey::Path(path) => {
                                match self.basalt.image_cache_ref().load_from_path(
                                    ImageCacheLifetime::Immeditate,
                                    (),
                                    path,
                                ) {
                                    Ok(image_info) => {
                                        (
                                            ImageSource::Cache(image_cache_key),
                                            Coords::new(
                                                image_info.width as f32,
                                                image_info.height as f32,
                                            ),
                                        )
                                    },
                                    Err(e) => {
                                        println!(
                                            "[Basalt]: Bin ID: {:?} | Failed to load image from \
                                             path, '{}': {}",
                                            self.id,
                                            path.display(),
                                            e
                                        );
                                        (ImageSource::None, Coords::new(0.0, 0.0))
                                    },
                                }
                            },
                            ImageCacheKey::Url(url) => {
                                match self.basalt.image_cache_ref().load_from_url(
                                    ImageCacheLifetime::Immeditate,
                                    (),
                                    url.as_str(),
                                ) {
                                    Ok(image_info) => {
                                        (
                                            ImageSource::Cache(image_cache_key),
                                            Coords::new(
                                                image_info.width as f32,
                                                image_info.height as f32,
                                            ),
                                        )
                                    },
                                    Err(e) => {
                                        println!(
                                            "[Basalt]: Bin ID: {:?} | Failed to load image from \
                                             url, '{}': {}",
                                            self.id, url, e
                                        );
                                        (ImageSource::None, Coords::new(0.0, 0.0))
                                    },
                                }
                            },
                            ImageCacheKey::Glyph(_) => {
                                println!(
                                    "[Basalt]: Bin ID: {:?} | Unable to use glyph cache key to \
                                     load image.",
                                    self.id,
                                );
                                (ImageSource::None, Coords::new(0.0, 0.0))
                            },
                            ImageCacheKey::User(..) => {
                                println!(
                                    "[Basalt]: Bin ID: {:?} | Unable to use user cache key to \
                                     load image.",
                                    self.id,
                                );
                                (ImageSource::None, Coords::new(0.0, 0.0))
                            },
                        }
                    },
                }
            },
            None => {
                match style.back_image_vk.clone() {
                    Some(image_vk) => {
                        let [w, h, _] = image_vk.extent();
                        (
                            ImageSource::Vulkano(image_vk),
                            Coords::new(w as f32, h as f32),
                        )
                    },
                    None => (ImageSource::None, Coords::new(0.0, 0.0)),
                }
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

        let back_img_vert_ty = match style.back_image_effect.as_ref() {
            Some(some) => some.vert_type(),
            None => 100,
        };

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.back_image = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // -- Opacity ------------------------------------------------------------------ //

        // -- Borders, Backround & Custom Verts --------------------------------------------- //

        let base_z = z_unorm(z_index);
        let content_z = z_unorm(z_index + 1);
        let mut verts = Vec::with_capacity(54);

        let mut border_color_t = style.border_color_t.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_b = style.border_color_b.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_l = style.border_color_l.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut border_color_r = style.border_color_r.clone().unwrap_or(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });

        let mut back_color = style.back_color.clone().unwrap_or(Color {
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

        if border_radius_tl != 0.0
            || border_radius_tr != 0.0
            || border_radius_bl != 0.0
            || border_radius_br != 0.0
        {
            let border_radius_tmax = if border_radius_tl > border_radius_tr {
                border_radius_tl
            } else {
                border_radius_tr
            };

            let border_radius_bmax = if border_radius_bl > border_radius_br {
                border_radius_bl
            } else {
                border_radius_br
            };

            if back_color.a > 0.0 || back_image_src != ImageSource::None {
                let mut back_verts = Vec::new();

                if border_radius_tl != 0.0 || border_radius_tr != 0.0 {
                    back_verts.push([bpu.tri[0] - border_radius_tr, bpu.tri[1]]);
                    back_verts.push([bpu.tli[0] + border_radius_tl, bpu.tli[1]]);
                    back_verts.push([
                        bpu.tli[0] + border_radius_tl,
                        bpu.tli[1] + border_radius_tmax,
                    ]);
                    back_verts.push([bpu.tri[0] - border_radius_tr, bpu.tri[1]]);
                    back_verts.push([
                        bpu.tli[0] + border_radius_tl,
                        bpu.tli[1] + border_radius_tmax,
                    ]);
                    back_verts.push([
                        bpu.tri[0] - border_radius_tr,
                        bpu.tri[1] + border_radius_tmax,
                    ]);

                    if border_radius_tl > border_radius_tr {
                        back_verts.push([bpu.tri[0], bpu.tri[1] + border_radius_tr]);
                        back_verts
                            .push([bpu.tri[0] - border_radius_tr, bpu.tri[1] + border_radius_tr]);
                        back_verts.push([
                            bpu.tri[0] - border_radius_tr,
                            bpu.tri[1] + border_radius_tmax,
                        ]);
                        back_verts.push([bpu.tri[0], bpu.tri[1] + border_radius_tr]);
                        back_verts.push([
                            bpu.tri[0] - border_radius_tr,
                            bpu.tri[1] + border_radius_tmax,
                        ]);
                        back_verts.push([bpu.tri[0], bpu.tri[1] + border_radius_tmax]);
                    } else if border_radius_tr > border_radius_tl {
                        back_verts
                            .push([bpu.tli[0] + border_radius_tl, bpu.tli[1] + border_radius_tl]);
                        back_verts.push([bpu.tli[0], bpu.tli[1] + border_radius_tl]);
                        back_verts.push([bpu.tli[0], bpu.tli[1] + border_radius_tmax]);
                        back_verts
                            .push([bpu.tli[0] + border_radius_tl, bpu.tli[1] + border_radius_tl]);
                        back_verts.push([bpu.tli[0], bpu.tli[1] + border_radius_tmax]);
                        back_verts.push([
                            bpu.tli[0] + border_radius_tl,
                            bpu.tli[1] + border_radius_tmax,
                        ]);
                    }
                }

                if border_radius_bl != 0.0 || border_radius_br != 0.0 {
                    back_verts.push([
                        bpu.bri[0] - border_radius_br,
                        bpu.bri[1] - border_radius_bmax,
                    ]);
                    back_verts.push([
                        bpu.bli[0] + border_radius_bl,
                        bpu.bli[1] - border_radius_bmax,
                    ]);
                    back_verts.push([bpu.bli[0] + border_radius_bl, bpu.bli[1]]);
                    back_verts.push([
                        bpu.bri[0] - border_radius_br,
                        bpu.bri[1] - border_radius_bmax,
                    ]);
                    back_verts.push([bpu.bli[0] + border_radius_bl, bpu.bli[1]]);
                    back_verts.push([bpu.bri[0] - border_radius_br, bpu.bri[1]]);

                    if border_radius_bl > border_radius_br {
                        back_verts.push([bpu.bri[0], bpu.bri[1] - border_radius_bmax]);
                        back_verts.push([
                            bpu.bri[0] - border_radius_br,
                            bpu.bri[1] - border_radius_bmax,
                        ]);
                        back_verts
                            .push([bpu.bri[0] - border_radius_br, bpu.bri[1] - border_radius_br]);
                        back_verts.push([bpu.bri[0], bpu.bri[1] - border_radius_bmax]);
                        back_verts
                            .push([bpu.bri[0] - border_radius_br, bpu.bri[1] - border_radius_br]);
                        back_verts.push([bpu.bri[0], bpu.bri[1] - border_radius_br]);
                    } else if border_radius_br > border_radius_bl {
                        back_verts.push([
                            bpu.bli[0] + border_radius_bl,
                            bpu.bli[1] - border_radius_bmax,
                        ]);
                        back_verts.push([bpu.bli[0], bpu.bli[1] - border_radius_bmax]);
                        back_verts.push([bpu.bli[0], bpu.bli[1] - border_radius_bl]);
                        back_verts.push([
                            bpu.bli[0] + border_radius_bl,
                            bpu.bli[1] - border_radius_bmax,
                        ]);
                        back_verts.push([bpu.bli[0], bpu.bli[1] - border_radius_bl]);
                        back_verts
                            .push([bpu.bli[0] + border_radius_bl, bpu.bli[1] - border_radius_bl]);
                    }
                }

                if border_radius_tl != 0.0 {
                    let a = (bpu.tli[0], bpu.tli[1] + border_radius_tl);
                    let b = (bpu.tli[0], bpu.tli[1]);
                    let c = (bpu.tli[0] + border_radius_tl, bpu.tli[1]);
                    let dx = bpu.tli[0] + border_radius_tl;
                    let dy = bpu.tli[1] + border_radius_tl;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_tr != 0.0 {
                    let a = (bpu.tri[0], bpu.tri[1] + border_radius_tr);
                    let b = (bpu.tri[0], bpu.tri[1]);
                    let c = (bpu.tri[0] - border_radius_tr, bpu.tri[1]);
                    let dx = bpu.tri[0] - border_radius_tr;
                    let dy = bpu.tri[1] + border_radius_tr;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_bl != 0.0 {
                    let a = (bpu.bli[0], bpu.bli[1] - border_radius_bl);
                    let b = (bpu.bli[0], bpu.bli[1]);
                    let c = (bpu.bli[0] + border_radius_bl, bpu.bli[1]);
                    let dx = bpu.bli[0] + border_radius_bl;
                    let dy = bpu.bli[1] - border_radius_bl;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_br != 0.0 {
                    let a = (bpu.bri[0], bpu.bri[1] - border_radius_br);
                    let b = (bpu.bri[0], bpu.bri[1]);
                    let c = (bpu.bri[0] - border_radius_br, bpu.bri[1]);
                    let dx = bpu.bri[0] - border_radius_br;
                    let dy = bpu.bri[1] - border_radius_br;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                back_verts.push([bpu.tri[0], bpu.tri[1] + border_radius_tmax]);
                back_verts.push([bpu.tli[0], bpu.tli[1] + border_radius_tmax]);
                back_verts.push([bpu.bli[0], bpu.bli[1] - border_radius_bmax]);
                back_verts.push([bpu.tri[0], bpu.tri[1] + border_radius_tmax]);
                back_verts.push([bpu.bli[0], bpu.bli[1] - border_radius_bmax]);
                back_verts.push([bpu.bri[0], bpu.bri[1] - border_radius_bmax]);

                let ty = if back_image_src != ImageSource::None {
                    back_img_vert_ty
                } else {
                    0
                };

                let bc_tlwh = back_image_coords.tlwh();

                for [x, y] in back_verts {
                    let coords_x =
                        (((x - bpu.tli[0]) / (bpu.tri[0] - bpu.tli[0])) * bc_tlwh[2]) + bc_tlwh[0];
                    let coords_y =
                        (((y - bpu.tli[1]) / (bpu.bli[1] - bpu.tli[1])) * bc_tlwh[3]) + bc_tlwh[1];

                    verts.push(ItfVertInfo {
                        position: [x, y, base_z],
                        coords: [coords_x, coords_y],
                        color: back_color.as_array(),
                        ty,
                        tex_i: 0,
                    });
                }
            }
        } else {
            if border_color_t.a > 0.0 && border_size_t > 0.0 {
                // Top Border
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_b.a > 0.0 && border_size_b > 0.0 {
                // Bottom Border
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_l.a > 0.0 && border_size_l > 0.0 {
                // Left Border
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tlo[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.blo[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.blo[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_r.a > 0.0 && border_size_r > 0.0 {
                // Right Border
                verts.push(ItfVertInfo {
                    position: [bpu.tro[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tro[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bro[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_t.a > 0.0
                && border_size_t > 0.0
                && border_color_l.a > 0.0
                && border_size_l > 0.0
            {
                // Top Left Border Corner (Color of Left)
                verts.push(ItfVertInfo {
                    position: [bpu.tlo[0], bpu.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tlo[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Top Left Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tlo[0], bpu.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_t.a > 0.0
                && border_size_t > 0.0
                && border_color_r.a > 0.0
                && border_size_r > 0.0
            {
                // Top Right Border Corner (Color of Right)
                verts.push(ItfVertInfo {
                    position: [bpu.tro[0], bpu.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tro[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Top Right Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: [bpu.tro[0], bpu.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_b.a > 0.0
                && border_size_b > 0.0
                && border_color_l.a > 0.0
                && border_size_l > 0.0
            {
                // Bottom Left Border Corner (Color of Left)
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.blo[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.blo[0], bpu.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Bottom Left Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.blo[0], bpu.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_b.a > 0.0
                && border_size_b > 0.0
                && border_color_r.a > 0.0
                && border_size_r > 0.0
            {
                // Bottom Right Border Corner (Color of Right)
                verts.push(ItfVertInfo {
                    position: [bpu.bro[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bro[0], bpu.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Bottom Right Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bro[0], bpu.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if back_color.a > 0.0 || back_image_src != ImageSource::None {
                let ty = if back_image_src != ImageSource::None {
                    back_img_vert_ty
                } else {
                    0
                };

                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: back_image_coords.top_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tli[0], bpu.tli[1], base_z],
                    coords: back_image_coords.top_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: back_image_coords.bottom_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.tri[0], bpu.tri[1], base_z],
                    coords: back_image_coords.top_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bli[0], bpu.bli[1], base_z],
                    coords: back_image_coords.bottom_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bpu.bri[0], bpu.bri[1], base_z],
                    coords: back_image_coords.bottom_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
            }
        }

        let mut outer_vert_data: HashMap<ImageSource, Vec<ItfVertInfo>> = HashMap::new();
        outer_vert_data.insert(back_image_src, verts);
        let mut inner_vert_data: HashMap<ImageSource, Vec<ItfVertInfo>> = HashMap::new();

        inner_vert_data.insert(
            ImageSource::None,
            style
                .custom_verts
                .iter()
                .map(|vertex| {
                    let z = if vertex.position.2 == 0 {
                        content_z
                    } else {
                        z_unorm(vertex.position.2)
                    };

                    ItfVertInfo {
                        position: [left + vertex.position.0, top + vertex.position.1, z],
                        coords: [0.0, 0.0],
                        color: vertex.color.as_array(),
                        ty: 0,
                        tex_i: 0,
                    }
                })
                .collect(),
        );

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.back_vertex = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // -- Text -------------------------------------------------------------------------- //

        'text_done: {
            if style.text.is_empty() {
                break 'text_done;
            }

            // -- Configure -- //

            let text_height = style.text_height.unwrap_or(12.0) * context.scale;
            let pad_t = style.pad_t.unwrap_or(0.0);
            let pad_b = style.pad_b.unwrap_or(0.0);
            let pad_l = style.pad_l.unwrap_or(0.0);
            let pad_r = style.pad_r.unwrap_or(0.0);

            let line_height = match style.line_spacing {
                Some(spacing) => text_height + spacing,
                None => text_height * 1.2,
            };

            let metrics = text::Metrics {
                font_size: text_height,
                line_height,
            };

            let mut buffer = text::Buffer::new(&mut context.font_system, metrics);
            let body_width = (bpu.tri[0] - bpu.tli[0] - pad_l - pad_r) * context.scale;
            let body_height = (bpu.bli[1] - bpu.tli[1] - pad_t - pad_b) * context.scale;

            let mut attrs = text::Attrs::new();
            let font_family = style
                .font_family
                .clone()
                .or_else(|| context.default_font.family.clone());
            let font_weight = style.font_weight.or(context.default_font.weight);
            let font_stretch = style.font_stretch.or(context.default_font.strench);
            let font_style = style.font_style.or(context.default_font.style);

            if let Some(font_family) = font_family.as_ref() {
                attrs = attrs.family(text::Family::Name(font_family));
            }

            if let Some(font_weight) = font_weight {
                attrs = attrs.weight(font_weight.into());
            }

            if let Some(font_stretch) = font_stretch {
                attrs = attrs.stretch(font_stretch.into());
            }

            if let Some(font_style) = font_style {
                attrs = attrs.style(font_style.into());
            }

            let text_style = TextStyle {
                text_height,
                line_height,
                body_width,
                body_height,
                wrap: style.text_wrap.unwrap_or(TextWrap::Normal),
                vert_align: style.text_vert_align.unwrap_or(TextVertAlign::Top),
                hori_align: style.text_hori_align.unwrap_or(TextHoriAlign::Left),
                font_family: font_family.clone(),
                font_weight,
                font_stretch,
                font_style,
            };

            let body_from_t = bpu.tli[1] + pad_t;
            let body_from_l = bpu.tli[0] + pad_l;

            if let Some(last_text_state) = bpu.text_state.as_mut() {
                if last_text_state.text == style.text && last_text_state.style == text_style {
                    if last_text_state.body_from_t == body_from_t
                        && last_text_state.body_from_l == body_from_l
                    {
                        inner_vert_data.extend(
                            last_text_state.vertex_data.clone().into_iter().map(
                                |(image_cache_key, vertexes)| {
                                    (ImageSource::Cache(image_cache_key), vertexes)
                                },
                            ),
                        );
                    } else {
                        let translate_y = body_from_t - last_text_state.body_from_t;
                        let translate_x = body_from_l - last_text_state.body_from_l;

                        inner_vert_data.extend(last_text_state.vertex_data.iter_mut().map(
                            |(image_cache_key, vertexes)| {
                                vertexes.iter_mut().for_each(|vertex| {
                                    vertex.position[0] += translate_x;
                                    vertex.position[1] += translate_y;
                                });

                                (
                                    ImageSource::Cache(image_cache_key.clone()),
                                    vertexes.clone(),
                                )
                            },
                        ));

                        last_text_state.body_from_t = body_from_t;
                        last_text_state.body_from_l = body_from_l;
                    }

                    break 'text_done;
                }
            }

            if matches!(
                style.text_wrap,
                Some(TextWrap::Shift) | Some(TextWrap::None)
            ) {
                buffer.set_size(&mut context.font_system, f32::MAX, body_height);
            } else {
                buffer.set_size(&mut context.font_system, body_width, f32::MAX);
            }

            // -- Shaping -- //

            if style.text_secret == Some(true) {
                buffer.set_text(
                    &mut context.font_system,
                    &(0..style.text.len()).map(|_| '*').collect::<String>(),
                    attrs,
                    cosmic_text::Shaping::Advanced,
                );
            } else {
                buffer.set_text(
                    &mut context.font_system,
                    &style.text,
                    attrs,
                    cosmic_text::Shaping::Advanced,
                );
            }

            let mut image_cache_keys = HashSet::new();
            let mut min_line_y = None;
            let mut max_line_y = None;
            let mut glyph_info = Vec::new();

            // -- Layout -- //

            for run in buffer.layout_runs() {
                if run.line_i == 0 {
                    min_line_y = Some(run.line_y - text_height);
                }

                if max_line_y.is_none() || *max_line_y.as_ref().unwrap() < run.line_y {
                    max_line_y = Some(run.line_y);
                }

                // Note: TextWrap::Shift is handled normally, but when it overflows it behaves like
                //       TextHoriAlign::Right

                let text_hori_align =
                    if style.text_wrap == Some(TextWrap::Shift) && run.line_w > body_width {
                        Some(TextHoriAlign::Right)
                    } else {
                        style.text_hori_align
                    };

                // Note: Round not to interfere with hinting
                let hori_align_offset = match text_hori_align {
                    None | Some(TextHoriAlign::Left) => 0.0,
                    Some(TextHoriAlign::Center) => ((body_width - run.line_w) / 2.0).round(),
                    Some(TextHoriAlign::Right) => (body_width - run.line_w).round(),
                };

                for glyph in run.glyphs.iter() {
                    let glyph = glyph.physical((0.0, 0.0), 1.0);
                    let image_cache_key = ImageCacheKey::Glyph(glyph.cache_key);
                    image_cache_keys.insert(image_cache_key.clone());

                    glyph_info.push((
                        image_cache_key,
                        glyph.x as f32 + hori_align_offset,
                        run.line_y - ((line_height - text_height) / 2.0).floor(),
                    ));
                }
            }

            if glyph_info.is_empty() || image_cache_keys.is_empty() || min_line_y.is_none() {
                break 'text_done;
            }

            // -- Glyph Fetch/Raster -- //

            let image_cache_keys = image_cache_keys.into_iter().collect::<Vec<_>>();
            let mut image_infos = HashMap::new();
            let mut glyph_vertex_data = HashMap::new();

            // NOTE: All images that a Bin could use should be contained in the vertex data, so the
            // renderer doesn't remove them if they aren't displayed.

            for (image_info_op, image_cache_key) in self
                .basalt
                .image_cache_ref()
                .obtain_image_infos(image_cache_keys.clone())
                .into_iter()
                .zip(image_cache_keys.into_iter())
            {
                if let Some(image_info) = image_info_op {
                    image_infos.insert(image_cache_key.clone(), image_info);
                    glyph_vertex_data.insert(image_cache_key, Vec::new());
                    continue;
                }

                let swash_cache_id = match image_cache_key {
                    ImageCacheKey::Glyph(swash_cache_id) => swash_cache_id,
                    _ => unreachable!(),
                };

                if let Some(swash_image) = context
                    .glyph_cache
                    .get_image_uncached(&mut context.font_system, swash_cache_id)
                {
                    if swash_image.placement.width == 0
                        || swash_image.placement.height == 0
                        || swash_image.data.is_empty()
                    {
                        continue;
                    }

                    let (vertex_type, image_format): (i32, _) = match swash_image.content {
                        text::SwashContent::Mask => (2, ImageFormat::LMono),
                        text::SwashContent::SubpixelMask => (2, ImageFormat::LRGBA),
                        text::SwashContent::Color => (100, ImageFormat::LRGBA),
                    };

                    let image_info = self
                        .basalt
                        .image_cache_ref()
                        .load_raw_image(
                            image_cache_key.clone(),
                            ImageCacheLifetime::Indefinite,
                            image_format,
                            swash_image.placement.width,
                            swash_image.placement.height,
                            GlyphImageAssociatedData {
                                vertex_type,
                                placement_top: swash_image.placement.top,
                                placement_left: swash_image.placement.left,
                            },
                            ImageData::D8(swash_image.data.into_iter().collect()),
                        )
                        .unwrap();

                    image_infos.insert(image_cache_key.clone(), image_info);
                    glyph_vertex_data.insert(image_cache_key, Vec::new());
                }
            }

            // -- Finalize BinPlacement -- //

            let text_body_height = max_line_y.unwrap() - min_line_y.unwrap();

            let vert_align_offset = match style.text_vert_align {
                None | Some(TextVertAlign::Top) => 0.0,
                Some(TextVertAlign::Center) => ((body_height - text_body_height) / 2.0).round(),
                Some(TextVertAlign::Bottom) => (body_height - text_body_height).round(),
            };

            let mut color = style
                .text_color
                .clone()
                .unwrap_or_else(|| Color::srgb_hex("000000"));

            color.a *= opacity;

            for (image_cache_key, mut glyph_x, mut glyph_y) in glyph_info {
                let image_info = match image_infos.get(&image_cache_key) {
                    Some(coords) => coords.clone(),
                    None => continue,
                };

                let coords = Coords::new(image_info.width as f32, image_info.height as f32);
                let associated_data = image_info
                    .associated_data::<GlyphImageAssociatedData>()
                    .unwrap();
                glyph_y += vert_align_offset - associated_data.placement_top as f32;
                glyph_x += associated_data.placement_left as f32;
                let [glyph_w, glyph_h] = coords.width_height();
                let min_x = (glyph_x / context.scale) + pad_l + bpu.tli[0];
                let min_y = (glyph_y / context.scale) + pad_t + bpu.tli[1];
                let max_x = min_x + (glyph_w / context.scale);
                let max_y = min_y + (glyph_h / context.scale);
                let [c_min_x, c_min_y] = coords.top_left();
                let [c_max_x, c_max_y] = coords.bottom_right();

                // -- Vertex Generation -- //

                glyph_vertex_data
                    .get_mut(&image_cache_key)
                    .unwrap()
                    .append(&mut vec![
                        ItfVertInfo {
                            position: [max_x, min_y, content_z],
                            coords: [c_max_x, c_min_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [min_x, min_y, content_z],
                            coords: [c_min_x, c_min_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [min_x, max_y, content_z],
                            coords: [c_min_x, c_max_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [max_x, min_y, content_z],
                            coords: [c_max_x, c_min_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [min_x, max_y, content_z],
                            coords: [c_min_x, c_max_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [max_x, max_y, content_z],
                            coords: [c_max_x, c_max_y],
                            color: color.as_array(),
                            ty: associated_data.vertex_type,
                            tex_i: 0,
                        },
                    ]);
            }

            for (image_cache_key, vertexes) in &glyph_vertex_data {
                inner_vert_data
                    .entry(ImageSource::Cache(image_cache_key.clone()))
                    .or_default()
                    .extend_from_slice(vertexes);
            }

            bpu.text_state = Some(TextState {
                style: text_style,
                text: style.text.clone(),
                body_from_t,
                body_from_l,
                vertex_data: glyph_vertex_data,
            });
        }

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.text = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // -- Bounds Checks --------------------------------------------------------------------- //

        let mut vert_data = inner_vert_data.values_mut();
        let mut bounds = inner_bounds;

        for vdi in 0..2 {
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

            if vdi == 0 {
                vert_data = outer_vert_data.values_mut();
                bounds = outer_bounds;
            } else {
                break;
            }
        }

        let mut vert_data = inner_vert_data;

        for (image_source, mut vertexes) in outer_vert_data {
            vert_data.entry(image_source).or_default().append(&mut vertexes);
        }

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.overflow = inst.elapsed().as_micros() as f32 / 1000.0;
            *inst = Instant::now();
        }

        // ----------------------------------------------------------------------------- //

        for verts in vert_data.values_mut() {
            scale_verts(&context.extent, context.scale, verts);
            verts.shrink_to_fit();
        }

        if let Some((ref mut inst, _, ref mut metrics)) = metrics_op.as_mut() {
            metrics.vertex_scale = inst.elapsed().as_micros() as f32 / 1000.0;
        }

        let bpu = RwLockWriteGuard::downgrade(bpu);
        self.call_on_update_hooks(&bpu);

        (
            vert_data,
            metrics_op.take().map(|(inst, inst_total, mut metrics)| {
                metrics.post_update = inst.elapsed().as_micros() as f32 / 1000.0;
                metrics.total = inst_total.elapsed().as_micros() as f32 / 1000.0;
                metrics
            }),
        )
    }
}

fn z_unorm(z: i16) -> f32 {
    (z as f32 + i16::max_value() as f32) / u16::max_value() as f32
}

fn curve_line_segments(
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut len = 0.0;
    let mut lpt = a;
    let mut steps = 10;

    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let npt = (
            ((1.0 - t).powi(2) * a.0) + (2.0 * (1.0 - t) * t * b.0) + (t.powi(2) * c.0),
            ((1.0 - t).powi(2) * a.1) + (2.0 * (1.0 - t) * t * b.1) + (t.powi(2) * c.1),
        );

        len += ((lpt.0 - npt.0) + (lpt.1 - npt.1)).sqrt();
        lpt = npt;
    }

    steps = len.ceil() as usize;

    if steps < 3 {
        steps = 3;
    }

    lpt = a;
    let mut out = Vec::new();

    for s in 1..=steps {
        let t = s as f32 / steps as f32;
        let npt = (
            ((1.0 - t).powi(2) * a.0) + (2.0 * (1.0 - t) * t * b.0) + (t.powi(2) * c.0),
            ((1.0 - t).powi(2) * a.1) + (2.0 * (1.0 - t) * t * b.1) + (t.powi(2) * c.1),
        );

        out.push(((lpt.0, lpt.1), (npt.0, npt.1)));
        lpt = npt;
    }

    out
}
