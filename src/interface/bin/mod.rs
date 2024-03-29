pub mod style;
pub use self::style::{
    BinPosition, BinStyle, BinVert, Color, FontStretch, FontStyle, FontWeight, ImageEffect,
    TextHoriAlign, TextVertAlign, TextWrap,
};

/// An ID of a `Bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BinID(pub(super) u64);

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Barrier, Weak};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapAny;
use cosmic_text as text;
use parking_lot::{Mutex, RwLock};

use crate::atlas::{
    AtlasCacheCtrl, AtlasCoords, Image, ImageData, ImageDims, ImageType, SubImageCacheID,
};
use crate::image_view::BstImageView;
use crate::input::key::KeyCombo;
use crate::input::state::{LocalCursorState, LocalKeyState, WindowState};
use crate::input::{Char, InputHookCtrl, InputHookID, InputHookTarget, MouseButton};
pub use crate::interface::bin::style::BinStyleValidation;
use crate::interface::render::composer::UpdateContext;
use crate::interface::{scale_verts, ItfVertInfo};
use crate::interval::IntvlHookCtrl;
use crate::Basalt;

pub trait KeepAlive {}
impl KeepAlive for Arc<Bin> {}
impl KeepAlive for Bin {}
impl<T: KeepAlive> KeepAlive for Vec<T> {}

#[derive(Default, Debug, Clone, Copy)]
pub struct BinUpdateStats {
    pub t_total: Duration,
    pub t_hidden: Duration,
    pub t_ancestors: Duration,
    pub t_position: Duration,
    pub t_zindex: Duration,
    pub t_image: Duration,
    pub t_opacity: Duration,
    pub t_verts: Duration,
    pub t_overflow: Duration,
    pub t_scale: Duration,
    pub t_callbacks: Duration,
    pub t_style_obtain: Duration,
    pub t_upcheck: Duration,
    pub t_postset: Duration,
    pub t_locks: Duration,
    pub t_text: Duration,
    pub t_ilmenite: Duration,
}

impl BinUpdateStats {
    pub fn divide(self, amt: f32) -> Self {
        BinUpdateStats {
            t_total: self.t_total.div_f32(amt),
            t_hidden: self.t_hidden.div_f32(amt),
            t_ancestors: self.t_ancestors.div_f32(amt),
            t_position: self.t_position.div_f32(amt),
            t_zindex: self.t_zindex.div_f32(amt),
            t_image: self.t_image.div_f32(amt),
            t_opacity: self.t_opacity.div_f32(amt),
            t_verts: self.t_verts.div_f32(amt),
            t_overflow: self.t_overflow.div_f32(amt),
            t_scale: self.t_scale.div_f32(amt),
            t_callbacks: self.t_callbacks.div_f32(amt),
            t_style_obtain: self.t_style_obtain.div_f32(amt),
            t_upcheck: self.t_upcheck.div_f32(amt),
            t_postset: self.t_postset.div_f32(amt),
            t_locks: self.t_postset.div_f32(amt),
            t_text: self.t_text.div_f32(amt),
            t_ilmenite: self.t_ilmenite.div_f32(amt),
        }
    }

    pub fn average(stats: &Vec<BinUpdateStats>) -> BinUpdateStats {
        let len = stats.len();
        Self::sum(stats).divide(len as f32)
    }

    pub fn sum(stats: &Vec<BinUpdateStats>) -> BinUpdateStats {
        let mut t_total = Duration::new(0, 0);
        let mut t_hidden = Duration::new(0, 0);
        let mut t_ancestors = Duration::new(0, 0);
        let mut t_position = Duration::new(0, 0);
        let mut t_zindex = Duration::new(0, 0);
        let mut t_image = Duration::new(0, 0);
        let mut t_opacity = Duration::new(0, 0);
        let mut t_verts = Duration::new(0, 0);
        let mut t_overflow = Duration::new(0, 0);
        let mut t_scale = Duration::new(0, 0);
        let mut t_callbacks = Duration::new(0, 0);
        let mut t_style_obtain = Duration::new(0, 0);
        let mut t_upcheck = Duration::new(0, 0);
        let mut t_postset = Duration::new(0, 0);
        let mut t_locks = Duration::new(0, 0);
        let mut t_text = Duration::new(0, 0);
        let mut t_ilmenite = Duration::new(0, 0);

        for stat in stats {
            t_total += stat.t_total;
            t_hidden += stat.t_hidden;
            t_ancestors += stat.t_ancestors;
            t_position += stat.t_position;
            t_zindex += stat.t_zindex;
            t_image += stat.t_image;
            t_opacity += stat.t_opacity;
            t_verts += stat.t_verts;
            t_overflow += stat.t_overflow;
            t_scale += stat.t_scale;
            t_callbacks += stat.t_callbacks;
            t_style_obtain += stat.t_style_obtain;
            t_upcheck += stat.t_upcheck;
            t_postset += stat.t_postset;
            t_locks += stat.t_locks;
            t_text += stat.t_text;
            t_ilmenite += stat.t_ilmenite;
        }

        BinUpdateStats {
            t_total,
            t_hidden,
            t_ancestors,
            t_position,
            t_zindex,
            t_image,
            t_opacity,
            t_verts,
            t_overflow,
            t_scale,
            t_callbacks,
            t_style_obtain,
            t_upcheck,
            t_postset,
            t_locks,
            t_text,
            t_ilmenite,
        }
    }
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
    Updated(Box<dyn FnMut(&Arc<Bin>, &PostUpdate) + Send + 'static>),
    ChildrenAdded(Box<dyn FnMut(&Arc<Bin>, &Vec<Arc<Bin>>) + Send + 'static>),
    ChildrenRemoved(Box<dyn FnMut(&Arc<Bin>, &Vec<Weak<Bin>>) + Send + 'static>),
}

pub struct Bin {
    basalt: Arc<Basalt>,
    id: BinID,
    hrchy: ArcSwapAny<Arc<BinHrchy>>,
    style: ArcSwapAny<Arc<BinStyle>>,
    initial: Mutex<bool>,
    update: AtomicBool,
    verts: Mutex<VertexState>,
    post_update: RwLock<PostUpdate>,
    input_hook_ids: Mutex<Vec<InputHookID>>,
    keep_alive: Mutex<Vec<Arc<dyn KeepAlive + Send + Sync>>>,
    last_update: Mutex<Instant>,
    update_stats: Mutex<BinUpdateStats>,
    internal_hooks: Mutex<HashMap<InternalHookTy, Vec<InternalHookFn>>>,
}

impl PartialEq for Bin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.basalt, &other.basalt) && self.id == other.id
    }
}

impl Eq for Bin {}

#[derive(Default)]
struct VertexState {
    verts: Vec<(Vec<ItfVertInfo>, Option<Arc<BstImageView>>, u64)>,
    #[allow(dead_code)]
    atlas_coords_in_use: HashSet<AtlasCoords>,
}

#[derive(Clone, Default, Debug)]
pub struct PostUpdate {
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
    /// Minimum/Maximum of Y Content before overflow checks
    pub unbound_mm_y: [f32; 2],
    /// Minimum/Maximum of X Content before overflow checks
    pub unbound_mm_x: [f32; 2],
    /// Target Extent (Generally Window Size)
    pub extent: [u32; 2],
    /// UI Scale Used
    pub scale: f32,
    text_state: Option<TextState>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TextState {
    atlas_coords: Vec<AtlasCoords>,
    text: String,
    style: TextStyle,
    body_from_t: f32,
    body_from_l: f32,
    vertex_data: HashMap<u32, Vec<ItfVertInfo>>,
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

        self.basalt.interface_ref().composer_ref().unpark();
    }
}

impl std::fmt::Debug for Bin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bin").field(&self.id.0).finish()
    }
}

impl Bin {
    pub(crate) fn new(id: BinID, basalt: Arc<Basalt>) -> Arc<Self> {
        Arc::new(Bin {
            id,
            basalt,
            hrchy: ArcSwapAny::from(Arc::new(BinHrchy::default())),
            style: ArcSwapAny::new(Arc::new(BinStyle::default())),
            initial: Mutex::new(true),
            update: AtomicBool::new(false),
            verts: Mutex::new(VertexState::default()),
            post_update: RwLock::new(PostUpdate::default()),
            input_hook_ids: Mutex::new(Vec::new()),
            keep_alive: Mutex::new(Vec::new()),
            last_update: Mutex::new(Instant::now()),
            update_stats: Mutex::new(BinUpdateStats::default()),
            internal_hooks: Mutex::new(HashMap::from([
                (InternalHookTy::Updated, Vec::new()),
                (InternalHookTy::UpdatedOnce, Vec::new()),
                (InternalHookTy::ChildrenAdded, Vec::new()),
                (InternalHookTy::ChildrenRemoved, Vec::new()),
            ])),
        })
    }

    pub fn basalt(&self) -> Arc<Basalt> {
        self.basalt.clone()
    }

    pub fn basalt_ref(&self) -> &Arc<Basalt> {
        &self.basalt
    }

    pub fn update_stats(&self) -> BinUpdateStats {
        *self.update_stats.lock()
    }

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
    pub fn on_update<F: FnMut(&Arc<Bin>, &PostUpdate) + Send + 'static>(self: &Arc<Self>, func: F) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::Updated)
            .unwrap()
            .push(InternalHookFn::Updated(Box::new(func)));
    }

    #[inline]
    pub fn on_update_once<F: FnMut(&Arc<Bin>, &PostUpdate) + Send + 'static>(
        self: &Arc<Self>,
        func: F,
    ) {
        self.internal_hooks
            .lock()
            .get_mut(&InternalHookTy::UpdatedOnce)
            .unwrap()
            .push(InternalHookFn::Updated(Box::new(func)));
    }

    pub fn wait_for_update(self: &Arc<Self>) {
        let barrier = Arc::new(Barrier::new(2));
        let barrier_copy = barrier.clone();

        self.on_update_once(move |_, _| {
            barrier_copy.wait();
        });

        // TODO: deadlock potential: if a bin is created, this method is called, then that
        // bin is dropped before any updates, this method won't return.
        barrier.wait();
    }

    pub fn last_update(&self) -> Instant {
        *self.last_update.lock()
    }

    pub fn keep_alive(&self, thing: Arc<dyn KeepAlive + Send + Sync>) {
        self.keep_alive.lock().push(thing);
    }

    pub fn parent(&self) -> Option<Arc<Bin>> {
        self.hrchy
            .load_full()
            .parent
            .as_ref()
            .and_then(|v| v.upgrade())
    }

    pub fn children(&self) -> Vec<Arc<Bin>> {
        self.hrchy
            .load_full()
            .children
            .iter()
            .filter_map(|wk| wk.upgrade())
            .collect()
    }

    pub fn children_recursive(self: &Arc<Bin>) -> Vec<Arc<Bin>> {
        let mut out = Vec::new();
        let mut to_check = vec![self.clone()];

        while !to_check.is_empty() {
            let child = to_check.pop().unwrap();
            to_check.append(&mut child.children());
            out.push(child);
        }

        out
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

    pub fn add_drag_events(self: &Arc<Self>, target_op: Option<Arc<Bin>>) {
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
                .window(&self.basalt.window())
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

                    target.update_children();
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

    pub fn add_enter_text_events(self: &Arc<Self>) {
        self.on_character(move |target, _, c| {
            let this = target.into_bin().unwrap();
            let mut style = this.style_copy();
            c.modify_string(&mut style.text);
            this.style_update(style).expect_valid();
            Default::default()
        });
    }

    pub fn add_button_fade_events(self: &Arc<Self>) {
        // TODO: New Input

        /*let bin = Arc::downgrade(self);
        let focused = Arc::new(AtomicBool::new(false));
        let _focused = focused.clone();
        let previous = Arc::new(Mutex::new(None));
        let _previous = previous.clone();

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_press(
            MouseButton::Left,
            move |data| {
                if let InputHookData::Press {
                    mouse_x,
                    mouse_y,
                    ..
                } = data
                {
                    let bin = match bin.upgrade() {
                        Some(some) => some,
                        None => return InputHookCtrl::Remove,
                    };

                    if bin.mouse_inside(*mouse_x, *mouse_y)
                        && !_focused.swap(true, atomic::Ordering::Relaxed)
                    {
                        let mut copy = bin.style_copy();
                        *_previous.lock() = copy.opacity;
                        copy.opacity = Some(0.5);
                        bin.style_update(copy);
                        bin.update_children();
                    }
                }

                InputHookCtrl::Retain
            },
        ));

        let bin = Arc::downgrade(self);

        self.input_hook_ids.lock().push(self.basalt.input_ref().on_mouse_release(
            MouseButton::Left,
            move |_| {
                let bin = match bin.upgrade() {
                    Some(some) => some,
                    None => return InputHookCtrl::Remove,
                };

                if focused.swap(false, atomic::Ordering::Relaxed) {
                    let mut copy = bin.style_copy();
                    copy.opacity = *previous.lock();
                    bin.style_update(copy);
                    bin.update_children();
                }

                InputHookCtrl::Retain
            },
        ));*/
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
                bin.update_children();
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
                bin.update_children();
                step_i += 1;
                Default::default()
            });
    }

    pub fn calc_vert_overflow(self: &Arc<Bin>) -> f32 {
        let self_post_up = self.post_update.read();
        let display_min = self_post_up.tli[1];
        let display_max = self_post_up.bli[1];
        let mut content_min = self_post_up.unbound_mm_y[0];
        let mut content_max = self_post_up.unbound_mm_y[1];

        for child in self.children() {
            let child_post_up = child.post_update.read();
            content_min = content_min.min(child_post_up.unbound_mm_y[0]);
            content_max = content_max.max(child_post_up.unbound_mm_y[1]);
        }

        let overflow_top = display_min - content_min;
        let overflow_bottom = content_max - display_max + self.style().pad_b.unwrap_or(0.0);

        overflow_top + overflow_bottom
    }

    pub fn calc_hori_overflow(self: &Arc<Bin>) -> f32 {
        let self_post_up = self.post_update.read();
        let display_min = self_post_up.tli[0];
        let display_max = self_post_up.tri[0];
        let mut content_min = self_post_up.unbound_mm_x[0];
        let mut content_max = self_post_up.unbound_mm_x[1];

        for child in self.children() {
            let child_post_up = child.post_update.read();
            content_min = content_min.min(child_post_up.unbound_mm_x[0]);
            content_max = content_max.max(child_post_up.unbound_mm_x[1]);
        }

        let overflow_left = display_min - content_min;
        let overflow_right = content_max - display_max + self.style().pad_r.unwrap_or(0.0);

        overflow_left + overflow_right
    }

    pub fn post_update(&self) -> PostUpdate {
        self.post_update.read().clone()
    }

    pub fn id(&self) -> BinID {
        self.id
    }

    pub fn mouse_inside(&self, mouse_x: f32, mouse_y: f32) -> bool {
        if self.is_hidden(None) {
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

    fn pos_size_tlwh(&self, win_size_: Option<[f32; 2]>) -> (f32, f32, f32, f32) {
        let win_size = win_size_.unwrap_or([0.0, 0.0]);
        let style = self.style();

        if *self.initial.lock() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let (par_t, par_b, par_l, par_r) = match style.position.unwrap_or(BinPosition::Window) {
            BinPosition::Window => (0.0, win_size[1], 0.0, win_size[0]),
            BinPosition::Parent => {
                match self.parent() {
                    Some(ref parent) => {
                        let (top, left, width, height) = parent.pos_size_tlwh(win_size_);
                        (top, top + height, left, left + width)
                    },
                    None => (0.0, win_size[1], 0.0, win_size[0]),
                }
            },
            BinPosition::Floating => {
                let parent = match self.parent() {
                    Some(some) => some,
                    None => {
                        // Only reachable if validation is unsafely bypassed.
                        unreachable!("No parent on floating Bin")
                    },
                };

                let (parent_t, parent_l, parent_w, parent_h) = parent.pos_size_tlwh(win_size_);
                let parent_style = parent.style_copy();
                let parent_pad_t = parent_style.pad_t.unwrap_or(0.0);
                let parent_pad_b = parent_style.pad_b.unwrap_or(0.0);
                let parent_pad_l = parent_style.pad_l.unwrap_or(0.0);
                let parent_pad_r = parent_style.pad_r.unwrap_or(0.0);
                let usable_width = parent_w - parent_pad_l - parent_pad_r;
                let usable_height = parent_h - parent_pad_t - parent_pad_b;

                struct Sibling {
                    order: u64,
                    width: f32,
                    height: f32,
                    margin_t: f32,
                    margin_b: f32,
                    margin_l: f32,
                    margin_r: f32,
                }

                let mut sibling_order = 0;
                let mut order_op = None;
                let mut siblings = Vec::new();

                // TODO: All siblings are recorded atm, this leaves room to override order in
                // the future, but for now order is just the order the bins are added to the
                // parent.

                for sibling in parent.children().into_iter() {
                    if sibling.id() == self.id {
                        order_op = Some(sibling_order);
                        sibling_order += 1;
                        continue;
                    }

                    let sibling_style = sibling.style_copy();

                    let mut sibling_width = match sibling_style.width {
                        Some(some) => some,
                        None => {
                            match sibling_style.width_pct {
                                Some(some) => some * usable_width,
                                None => {
                                    // Only reachable if validation is unsafely bypassed.
                                    unreachable!("'width' or 'width_pct' is not defined.")
                                },
                            }
                        },
                    };

                    let mut sibling_height = match sibling_style.height {
                        Some(some) => some,
                        None => {
                            match sibling_style.height_pct {
                                Some(some) => some * usable_height,
                                None => {
                                    // Only reachable if validation is unsafely bypassed.
                                    unreachable!("'height' or 'height_pct' is not defined.")
                                },
                            }
                        },
                    };

                    sibling_width += sibling_style.width_offset.unwrap_or(0.0);
                    sibling_height += sibling_style.height_offset.unwrap_or(0.0);

                    siblings.push(Sibling {
                        order: sibling_order,
                        width: sibling_width,
                        height: sibling_height,
                        margin_t: sibling_style.margin_t.unwrap_or(0.0),
                        margin_b: sibling_style.margin_b.unwrap_or(0.0),
                        margin_l: sibling_style.margin_l.unwrap_or(0.0),
                        margin_r: sibling_style.margin_r.unwrap_or(0.0),
                    });

                    sibling_order += 1;
                }

                assert!(order_op.is_some(), "Bin is not a child of parent.");

                let order = order_op.unwrap();
                let mut current_x = 0.0;
                let mut current_y = 0.0;
                let mut row_height = 0.0;
                let mut row_items = 0;

                for sibling in siblings {
                    if sibling.order > order {
                        break;
                    }

                    let add_width = sibling.margin_l + sibling.width + sibling.margin_r;
                    let height = sibling.margin_t + sibling.height + sibling.margin_b;

                    if add_width >= usable_width {
                        if row_items > 0 {
                            current_y += row_height;
                            row_items = 0;
                        }

                        current_x = 0.0;
                        current_y += height;
                    } else if current_x + add_width >= usable_width {
                        if row_items > 0 {
                            current_y += row_height;
                            row_items = 0;
                        }

                        current_x = add_width;
                        row_height = height;
                    } else {
                        current_x += add_width;

                        if height > row_height {
                            row_height = height;
                        }
                    }

                    row_items += 1;
                }

                let mut width = match style.width {
                    Some(some) => some,
                    None => {
                        match style.width_pct {
                            Some(some) => (some / 100.0) * usable_width,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("'width' or 'width_pct' is not defined.")
                            },
                        }
                    },
                };

                let mut height = match style.height {
                    Some(some) => some,
                    None => {
                        match style.height_pct {
                            Some(some) => (some / 100.0) * usable_height,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("'height' or 'height_pct' is not defined.")
                            },
                        }
                    },
                };

                width += style.width_offset.unwrap_or(0.0);
                height += style.height_offset.unwrap_or(0.0);
                let margin_l = style.margin_l.unwrap_or(0.0);
                let margin_r = style.margin_r.unwrap_or(0.0);
                let margin_t = style.margin_t.unwrap_or(0.0);
                let add_width = margin_l + width + margin_r;

                if current_x + add_width >= usable_width {
                    if row_items > 0 {
                        current_y += row_height;
                    }

                    let top = parent_t + parent_pad_t + current_y + margin_t;
                    let left = parent_l + parent_pad_l + margin_l;
                    return (top, left, width, height);
                }

                let top = parent_t + parent_pad_t + margin_t + current_y;
                let left = parent_l + parent_pad_l + margin_l + current_x;
                return (top, left, width, height);
            },
        };

        let pos_from_t = match style.pos_from_t {
            Some(some) => Some(some),
            None => {
                style
                    .pos_from_t_pct
                    .map(|some| (some / 100.0) * (par_b - par_t))
            },
        };

        let pos_from_b = match style.pos_from_b {
            Some(some) => Some(some),
            None => {
                style
                    .pos_from_b_pct
                    .map(|some| (some / 100.0) * (par_b - par_t))
            },
        }
        .map(|v| v + style.pos_from_b_offset.unwrap_or(0.0));

        let pos_from_l = match style.pos_from_l {
            Some(some) => Some(some),
            None => {
                style
                    .pos_from_l_pct
                    .map(|some| (some / 100.0) * (par_r - par_l))
            },
        };

        let pos_from_r = match style.pos_from_r {
            Some(some) => Some(some),
            None => {
                style
                    .pos_from_r_pct
                    .map(|some| (some / 100.0) * (par_r - par_l))
            },
        }
        .map(|v| v + style.pos_from_r_offset.unwrap_or(0.0));

        let from_t = match pos_from_t {
            Some(from_t) => par_t + from_t,
            None => {
                match pos_from_b {
                    Some(from_b) => {
                        match style.height {
                            Some(height) => par_b - from_b - height,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("Invalid position/dimension.")
                            },
                        }
                    },
                    None => {
                        // Only reachable if validation is unsafely bypassed.
                        unreachable!("Invalid position/dimension.")
                    },
                }
            },
        } + style.pos_from_t_offset.unwrap_or(0.0);

        let from_l = match pos_from_l {
            Some(from_l) => from_l + par_l,
            None => {
                match pos_from_r {
                    Some(from_r) => {
                        match style.width {
                            Some(width) => par_r - from_r - width,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("Invalid position/dimension.")
                            },
                        }
                    },
                    None => {
                        // Only reachable if validation is unsafely bypassed.
                        unreachable!("Invalid position/dimension.")
                    },
                }
            },
        } + style.pos_from_l_offset.unwrap_or(0.0);

        let width_offset = style.width_offset.unwrap_or(0.0);
        let width = {
            if let Some(pos_from_r) = pos_from_l.and(pos_from_r) {
                par_r - pos_from_r - from_l
            } else {
                match style.width {
                    Some(some) => some + width_offset,
                    None => {
                        match style.width_pct {
                            Some(some) => ((some / 100.0) * (par_r - par_l)) + width_offset,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("Invalid position/dimension.")
                            },
                        }
                    },
                }
            }
        };

        let height_offset = style.height_offset.unwrap_or(0.0);
        let height = {
            if let Some(pos_from_b) = pos_from_t.and(pos_from_b) {
                par_b - pos_from_b - from_t
            } else {
                match style.height {
                    Some(some) => some + height_offset,
                    None => {
                        match style.height_pct {
                            Some(some) => ((some / 100.0) * (par_b - par_t)) + height_offset,
                            None => {
                                // Only reachable if validation is unsafely bypassed.
                                unreachable!("Invalid position/dimension.")
                            },
                        }
                    },
                }
            }
        };

        (from_t, from_l, width, height)
    }

    pub fn visible(&self) -> bool {
        !self.is_hidden(None)
    }

    pub fn toggle_hidden(&self) {
        let mut style = self.style_copy();
        style.hidden = Some(!style.hidden.unwrap_or(false));
        self.style_update(style).expect_valid();
    }

    fn is_hidden(&self, style_: Option<&BinStyle>) -> bool {
        match match style_ {
            Some(style) => style.hidden.unwrap_or(false),
            None => self.style().hidden.unwrap_or(false),
        } {
            true => true,
            false => {
                match self.parent() {
                    Some(parent) => parent.is_hidden(None),
                    None => false,
                }
            },
        }
    }

    pub(crate) fn verts_cp(&self) -> Vec<(Vec<ItfVertInfo>, Option<Arc<BstImageView>>, u64)> {
        self.verts.lock().verts.clone()
    }

    pub(crate) fn wants_update(&self) -> bool {
        self.update.load(atomic::Ordering::SeqCst)
    }

    pub(crate) fn do_update(self: &Arc<Self>, context: &mut UpdateContext) {
        // -- Update Check ------------------------------------------------------------------ //

        let update_stats = self.basalt.show_bin_stats();
        let mut stats = BinUpdateStats::default();
        let mut inst = Instant::now();

        if *self.initial.lock() {
            return;
        }

        self.update.store(false, atomic::Ordering::SeqCst);

        if update_stats {
            stats.t_upcheck = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Style Obtain ------------------------------------------------------------------ //

        let style = self.style();
        let last_update = self.post_update();

        let scaled_win_size = [
            context.extent[0] / context.scale,
            context.extent[1] / context.scale,
        ];

        if update_stats {
            stats.t_style_obtain = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Hidden Check ------------------------------------------------------------------ //

        if self.is_hidden(Some(&style)) {
            *self.verts.lock() = VertexState::default();
            *self.last_update.lock() = Instant::now();
            // TODO: should the entire PostUpdate be reset?
            self.post_update.write().text_state = None;
            return;
        }

        if update_stats {
            stats.t_hidden = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Ancestors Obtain -------------------------------------------------------------- //

        let ancestor_data: Vec<(Arc<Bin>, Arc<BinStyle>, f32, f32, f32, f32)> = self
            .ancestors()
            .into_iter()
            .map(|bin| {
                let (top, left, width, height) = bin.pos_size_tlwh(Some(scaled_win_size));
                (bin.clone(), bin.style(), top, left, width, height)
            })
            .collect();

        if update_stats {
            stats.t_ancestors = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Position Calculation ---------------------------------------------------------- //

        let (top, left, width, height) = self.pos_size_tlwh(Some(scaled_win_size));
        let border_size_t = style.border_size_t.unwrap_or(0.0);
        let border_size_b = style.border_size_b.unwrap_or(0.0);
        let border_size_l = style.border_size_l.unwrap_or(0.0);
        let border_size_r = style.border_size_r.unwrap_or(0.0);

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

        if update_stats {
            stats.t_position = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- z-index calc ------------------------------------------------------------------ //

        let z_index = match style.z_index.as_ref() {
            Some(some) => *some,
            None => {
                let mut z_index_op = None;
                let mut checked = 0;

                for (_, check_style, ..) in &ancestor_data {
                    match check_style.z_index.as_ref() {
                        Some(some) => {
                            z_index_op = Some(*some + checked + 1);
                            break;
                        },
                        None => {
                            checked += 1;
                        },
                    }
                }

                z_index_op.unwrap_or(ancestor_data.len() as i16)
            },
        } + style.add_z_index.unwrap_or(0);

        if update_stats {
            stats.t_zindex = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- create post update ------------------------------------------------------- //

        let mut bps = PostUpdate {
            tlo: [left - border_size_l, top - border_size_t],
            tli: [left, top],
            blo: [left - border_size_l, top + height + border_size_b],
            bli: [left, top + height],
            tro: [left + width + border_size_r, top - border_size_t],
            tri: [left + width, top],
            bro: [left + width + border_size_r, top + height + border_size_b],
            bri: [left + width, top + height],
            z_index,
            unbound_mm_y: [top, top + height],
            unbound_mm_x: [left, left + width],
            text_state: None,
            extent: [
                context.extent[0].trunc() as u32,
                context.extent[1].trunc() as u32,
            ],
            scale: context.scale,
        };

        if update_stats {
            stats.t_postset = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Background Image --------------------------------------------------------- //

        let back_image_cache = style.back_image_cache.unwrap_or_default();

        let (back_img, back_coords) = match style.back_image.as_ref() {
            Some(path) => {
                match self.basalt.atlas_ref().load_image_from_path(
                    back_image_cache,
                    path,
                    Vec::new(),
                ) {
                    Ok(coords) => (None, coords),
                    Err(e) => {
                        // TODO: Check during validation
                        println!(
                            "[Basalt]: Bin ID: {:?} | failed to load image into atlas {}: {}",
                            self.id, path, e
                        );
                        (None, AtlasCoords::none())
                    },
                }
            },
            None => {
                match style.back_image_url.as_ref() {
                    Some(url) => {
                        match self.basalt.atlas_ref().load_image_from_url(
                            back_image_cache,
                            url,
                            Vec::new(),
                        ) {
                            Ok(coords) => (None, coords),
                            Err(e) => {
                                // TODO: Check during validation
                                println!(
                                    "[Basalt]: Bin ID: {:?} | failed to load image into atlas {}: \
                                     {}",
                                    self.id, url, e
                                );
                                (None, AtlasCoords::none())
                            },
                        }
                    },
                    None => {
                        match style.back_image_atlas.clone() {
                            Some(coords) => (None, coords),
                            None => {
                                match style.back_image_raw.as_ref() {
                                    Some(image) => {
                                        let coords = match style.back_image_raw_coords.as_ref() {
                                            Some(some) => some.clone(),
                                            None => {
                                                let dims = image.dimensions();

                                                AtlasCoords::external(
                                                    0.0,
                                                    0.0,
                                                    dims.width() as f32,
                                                    dims.height() as f32,
                                                )
                                            },
                                        };

                                        (Some(image.clone()), coords)
                                    },
                                    None => (None, AtlasCoords::none()),
                                }
                            },
                        }
                    },
                }
            },
        };

        let back_img_vert_ty = match style.back_image_effect.as_ref() {
            Some(some) => some.vert_type(),
            None => 100,
        };

        if update_stats {
            stats.t_image = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Opacity ------------------------------------------------------------------ //

        let mut opacity = style.opacity.unwrap_or(1.0);

        for (_, check_style, ..) in &ancestor_data {
            opacity *= check_style.opacity.unwrap_or(1.0);
        }

        if opacity != 1.0 {
            border_color_t.a *= opacity;
            border_color_b.a *= opacity;
            border_color_l.a *= opacity;
            border_color_r.a *= opacity;
            back_color.a *= opacity;
        }

        if update_stats {
            stats.t_opacity = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Borders, Backround & Custom Verts --------------------------------------------- //

        let base_z = (-z_index as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
        let content_z =
            (-(z_index + 1) as i32 + i16::max_value() as i32) as f32 / i32::max_value() as f32;
        let mut verts = Vec::with_capacity(54);

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

            if back_color.a > 0.0 || !back_coords.is_none() || back_img.is_some() {
                let mut back_verts = Vec::new();

                if border_radius_tl != 0.0 || border_radius_tr != 0.0 {
                    back_verts.push([bps.tri[0] - border_radius_tr, bps.tri[1]]);
                    back_verts.push([bps.tli[0] + border_radius_tl, bps.tli[1]]);
                    back_verts.push([
                        bps.tli[0] + border_radius_tl,
                        bps.tli[1] + border_radius_tmax,
                    ]);
                    back_verts.push([bps.tri[0] - border_radius_tr, bps.tri[1]]);
                    back_verts.push([
                        bps.tli[0] + border_radius_tl,
                        bps.tli[1] + border_radius_tmax,
                    ]);
                    back_verts.push([
                        bps.tri[0] - border_radius_tr,
                        bps.tri[1] + border_radius_tmax,
                    ]);

                    if border_radius_tl > border_radius_tr {
                        back_verts.push([bps.tri[0], bps.tri[1] + border_radius_tr]);
                        back_verts
                            .push([bps.tri[0] - border_radius_tr, bps.tri[1] + border_radius_tr]);
                        back_verts.push([
                            bps.tri[0] - border_radius_tr,
                            bps.tri[1] + border_radius_tmax,
                        ]);
                        back_verts.push([bps.tri[0], bps.tri[1] + border_radius_tr]);
                        back_verts.push([
                            bps.tri[0] - border_radius_tr,
                            bps.tri[1] + border_radius_tmax,
                        ]);
                        back_verts.push([bps.tri[0], bps.tri[1] + border_radius_tmax]);
                    } else if border_radius_tr > border_radius_tl {
                        back_verts
                            .push([bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tl]);
                        back_verts.push([bps.tli[0], bps.tli[1] + border_radius_tl]);
                        back_verts.push([bps.tli[0], bps.tli[1] + border_radius_tmax]);
                        back_verts
                            .push([bps.tli[0] + border_radius_tl, bps.tli[1] + border_radius_tl]);
                        back_verts.push([bps.tli[0], bps.tli[1] + border_radius_tmax]);
                        back_verts.push([
                            bps.tli[0] + border_radius_tl,
                            bps.tli[1] + border_radius_tmax,
                        ]);
                    }
                }

                if border_radius_bl != 0.0 || border_radius_br != 0.0 {
                    back_verts.push([
                        bps.bri[0] - border_radius_br,
                        bps.bri[1] - border_radius_bmax,
                    ]);
                    back_verts.push([
                        bps.bli[0] + border_radius_bl,
                        bps.bli[1] - border_radius_bmax,
                    ]);
                    back_verts.push([bps.bli[0] + border_radius_bl, bps.bli[1]]);
                    back_verts.push([
                        bps.bri[0] - border_radius_br,
                        bps.bri[1] - border_radius_bmax,
                    ]);
                    back_verts.push([bps.bli[0] + border_radius_bl, bps.bli[1]]);
                    back_verts.push([bps.bri[0] - border_radius_br, bps.bri[1]]);

                    if border_radius_bl > border_radius_br {
                        back_verts.push([bps.bri[0], bps.bri[1] - border_radius_bmax]);
                        back_verts.push([
                            bps.bri[0] - border_radius_br,
                            bps.bri[1] - border_radius_bmax,
                        ]);
                        back_verts
                            .push([bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_br]);
                        back_verts.push([bps.bri[0], bps.bri[1] - border_radius_bmax]);
                        back_verts
                            .push([bps.bri[0] - border_radius_br, bps.bri[1] - border_radius_br]);
                        back_verts.push([bps.bri[0], bps.bri[1] - border_radius_br]);
                    } else if border_radius_br > border_radius_bl {
                        back_verts.push([
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bmax,
                        ]);
                        back_verts.push([bps.bli[0], bps.bli[1] - border_radius_bmax]);
                        back_verts.push([bps.bli[0], bps.bli[1] - border_radius_bl]);
                        back_verts.push([
                            bps.bli[0] + border_radius_bl,
                            bps.bli[1] - border_radius_bmax,
                        ]);
                        back_verts.push([bps.bli[0], bps.bli[1] - border_radius_bl]);
                        back_verts
                            .push([bps.bli[0] + border_radius_bl, bps.bli[1] - border_radius_bl]);
                    }
                }

                if border_radius_tl != 0.0 {
                    let a = (bps.tli[0], bps.tli[1] + border_radius_tl);
                    let b = (bps.tli[0], bps.tli[1]);
                    let c = (bps.tli[0] + border_radius_tl, bps.tli[1]);
                    let dx = bps.tli[0] + border_radius_tl;
                    let dy = bps.tli[1] + border_radius_tl;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_tr != 0.0 {
                    let a = (bps.tri[0], bps.tri[1] + border_radius_tr);
                    let b = (bps.tri[0], bps.tri[1]);
                    let c = (bps.tri[0] - border_radius_tr, bps.tri[1]);
                    let dx = bps.tri[0] - border_radius_tr;
                    let dy = bps.tri[1] + border_radius_tr;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_bl != 0.0 {
                    let a = (bps.bli[0], bps.bli[1] - border_radius_bl);
                    let b = (bps.bli[0], bps.bli[1]);
                    let c = (bps.bli[0] + border_radius_bl, bps.bli[1]);
                    let dx = bps.bli[0] + border_radius_bl;
                    let dy = bps.bli[1] - border_radius_bl;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                if border_radius_br != 0.0 {
                    let a = (bps.bri[0], bps.bri[1] - border_radius_br);
                    let b = (bps.bri[0], bps.bri[1]);
                    let c = (bps.bri[0] - border_radius_br, bps.bri[1]);
                    let dx = bps.bri[0] - border_radius_br;
                    let dy = bps.bri[1] - border_radius_br;

                    for ((ax, ay), (bx, by)) in curve_line_segments(a, b, c) {
                        back_verts.push([dx, dy]);
                        back_verts.push([bx, by]);
                        back_verts.push([ax, ay]);
                    }
                }

                back_verts.push([bps.tri[0], bps.tri[1] + border_radius_tmax]);
                back_verts.push([bps.tli[0], bps.tli[1] + border_radius_tmax]);
                back_verts.push([bps.bli[0], bps.bli[1] - border_radius_bmax]);
                back_verts.push([bps.tri[0], bps.tri[1] + border_radius_tmax]);
                back_verts.push([bps.bli[0], bps.bli[1] - border_radius_bmax]);
                back_verts.push([bps.bri[0], bps.bri[1] - border_radius_bmax]);

                let ty = if !back_coords.is_none() || back_img.is_some() {
                    back_img_vert_ty
                } else {
                    0
                };

                let bc_tlwh = back_coords.tlwh();

                for [x, y] in back_verts {
                    let coords_x =
                        (((x - bps.tli[0]) / (bps.tri[0] - bps.tli[0])) * bc_tlwh[2]) + bc_tlwh[0];
                    let coords_y =
                        (((y - bps.tli[1]) / (bps.bli[1] - bps.tli[1])) * bc_tlwh[3]) + bc_tlwh[1];

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
                    position: [bps.tri[0], bps.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_b.a > 0.0 && border_size_b > 0.0 {
                // Bottom Border
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_l.a > 0.0 && border_size_l > 0.0 {
                // Left Border
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tlo[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.blo[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.blo[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if border_color_r.a > 0.0 && border_size_r > 0.0 {
                // Right Border
                verts.push(ItfVertInfo {
                    position: [bps.tro[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tro[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bro[0], bps.bri[1], base_z],
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
                    position: [bps.tlo[0], bps.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tlo[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Top Left Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tlo[0], bps.tlo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
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
                    position: [bps.tro[0], bps.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tro[0], bps.tri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Top Right Border Corner (Color of Top)
                verts.push(ItfVertInfo {
                    position: [bps.tro[0], bps.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_t.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
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
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.blo[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.blo[0], bps.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_l.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Bottom Left Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.blo[0], bps.blo[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.blo[1], base_z],
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
                    position: [bps.bro[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bro[0], bps.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_r.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                // Bottom Right Border Corner (Color of Bottom)
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bro[0], bps.bro[1], base_z],
                    coords: [0.0, 0.0],
                    color: border_color_b.as_array(),
                    ty: 0,
                    tex_i: 0,
                });
            }
            if back_color.a > 0.0 || !back_coords.is_none() || back_img.is_some() {
                let ty = if !back_coords.is_none() || back_img.is_some() {
                    back_img_vert_ty
                } else {
                    0
                };

                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
                    coords: back_coords.top_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tli[0], bps.tli[1], base_z],
                    coords: back_coords.top_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: back_coords.bottom_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.tri[0], bps.tri[1], base_z],
                    coords: back_coords.top_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bli[0], bps.bli[1], base_z],
                    coords: back_coords.bottom_left(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
                verts.push(ItfVertInfo {
                    position: [bps.bri[0], bps.bri[1], base_z],
                    coords: back_coords.bottom_right(),
                    color: back_color.as_array(),
                    ty,
                    tex_i: 0,
                });
            }
        }

        for BinVert {
            position,
            color,
        } in &style.custom_verts
        {
            let z = if position.2 == 0 {
                content_z
            } else {
                (-(z_index + position.2) as i32 + i16::max_value() as i32) as f32
                    / i32::max_value() as f32
            };

            verts.push(ItfVertInfo {
                position: [bps.tli[0] + position.0, bps.tli[1] + position.1, z],
                coords: [0.0, 0.0],
                color: color.as_array(),
                ty: 0,
                tex_i: 0,
            });
        }

        let mut vert_data = vec![(verts, back_img, back_coords.image_id())];
        let mut atlas_coords_in_use = HashSet::new();

        if !back_coords.is_none() && !back_coords.is_external() {
            atlas_coords_in_use.insert(back_coords);
        }

        if update_stats {
            stats.t_verts = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Text -------------------------------------------------------------------------- //

        'text_done: {
            if style.text.is_empty() {
                break 'text_done;
            }

            // -- Configure -- //

            let text_height = style.text_height.unwrap_or(12.0) * context.scale;

            let line_height = match style.line_spacing {
                Some(spacing) => text_height + spacing,
                None => text_height * 1.2,
            };

            let metrics = text::Metrics {
                font_size: text_height,
                line_height,
            };

            let mut buffer = text::Buffer::new(&mut context.font_system, metrics);
            let pad_t = style.pad_t.unwrap_or(0.0);
            let pad_b = style.pad_b.unwrap_or(0.0);
            let pad_l = style.pad_l.unwrap_or(0.0);
            let pad_r = style.pad_r.unwrap_or(0.0);
            let body_width = (bps.tri[0] - bps.tli[0] - pad_l - pad_r) * context.scale;
            let body_height = (bps.bli[1] - bps.tli[1] - pad_t - pad_b) * context.scale;

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

            let body_from_t = bps.tli[1] + pad_t;
            let body_from_l = bps.tli[0] + pad_l;

            if let Some(mut last_text_state) = last_update.text_state {
                if last_text_state.text == style.text && last_text_state.style == text_style {
                    if last_text_state.body_from_t == body_from_t
                        && last_text_state.body_from_l == body_from_l
                    {
                        for (tex_i, vertexes) in last_text_state.vertex_data.clone() {
                            vert_data.push((vertexes, None, tex_i as u64));
                        }

                        bps.text_state = Some(last_text_state);
                    } else {
                        let translate_y = body_from_t - last_text_state.body_from_t;
                        let translate_x = body_from_l - last_text_state.body_from_l;

                        for (tex_i, vertexes) in &mut last_text_state.vertex_data {
                            for vertex in vertexes.iter_mut() {
                                vertex.position[0] += translate_x;
                                vertex.position[1] += translate_y;
                            }

                            vert_data.push((vertexes.clone(), None, *tex_i as u64));
                        }

                        last_text_state.body_from_t = body_from_t;
                        last_text_state.body_from_l = body_from_l;
                        bps.text_state = Some(last_text_state);
                    }

                    break 'text_done;
                }
            }

            if matches!(
                style.text_wrap,
                Some(TextWrap::Shift) | Some(TextWrap::None)
            ) {
                buffer.set_size(&mut context.font_system, f32::MAX, body_height);
            } else if style.overflow_y == Some(true) {
                buffer.set_size(&mut context.font_system, body_width, f32::MAX);
            } else {
                buffer.set_size(&mut context.font_system, body_width, body_height);
            }

            // -- Shaping -- //

            if style.text_secret == Some(true) {
                buffer.set_text(
                    &mut context.font_system,
                    &(0..style.text.len()).map(|_| '*').collect::<String>(),
                    attrs,
                );
            } else {
                buffer.set_text(&mut context.font_system, &style.text, attrs);
            }

            let shape_lines = match style.line_limit {
                Some(limit) => limit.clamp(0, i32::max_value() as usize) as i32,
                None => i32::max_value(),
            };

            let num_lines = buffer.shape_until(&mut context.font_system, shape_lines);
            let mut atlas_cache_ids = HashSet::new();
            let mut min_line_y = None;
            let mut max_line_y = None;
            let mut glyph_info = Vec::new();

            // -- Layout -- //

            // Note: this iterator only covers visible lines
            for run in buffer.layout_runs() {
                if run.line_i == 0 {
                    min_line_y = Some(run.line_y - text_height);
                } else if run.line_i == num_lines as usize - 1 {
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
                    let atlas_cache_key = SubImageCacheID::Glyph(glyph.cache_key);
                    atlas_cache_ids.insert(atlas_cache_key.clone());

                    glyph_info.push((
                        atlas_cache_key,
                        glyph.x_int as f32 + hori_align_offset,
                        run.line_y - ((line_height - text_height) / 2.0).floor(),
                    ));
                }
            }

            if glyph_info.is_empty()
                || atlas_cache_ids.is_empty()
                || min_line_y.is_none()
                || num_lines == 0
            {
                break 'text_done;
            }

            // -- Glyph Fetch/Raster -- //

            let atlas_cache_ids = atlas_cache_ids.into_iter().collect::<Vec<_>>();
            let mut atlas_coords = HashMap::new();

            for (atlas_coords_op, atlas_cache_id) in self
                .basalt
                .atlas_ref()
                .batch_cache_coords(atlas_cache_ids.clone())
                .into_iter()
                .zip(atlas_cache_ids.into_iter())
            {
                if let Some(coords) = atlas_coords_op {
                    atlas_coords.insert(atlas_cache_id, coords);
                    continue;
                }

                let swash_cache_id = match atlas_cache_id {
                    SubImageCacheID::Glyph(swash_cache_id) => swash_cache_id,
                    _ => unreachable!(),
                };

                if let Some(swash_image) = context
                    .swash_cache
                    .get_image_uncached(&mut context.font_system, swash_cache_id)
                {
                    if swash_image.placement.width == 0
                        || swash_image.placement.height == 0
                        || swash_image.data.is_empty()
                    {
                        continue;
                    }

                    let (vertex_ty, image_ty): (i32, _) = match swash_image.content {
                        text::SwashContent::Mask => (2, ImageType::LMono),
                        text::SwashContent::SubpixelMask => (2, ImageType::LRGBA),
                        text::SwashContent::Color => (100, ImageType::LRGBA),
                    };

                    let atlas_image = Image::new(
                        image_ty,
                        ImageDims {
                            w: swash_image.placement.width,
                            h: swash_image.placement.height,
                        },
                        ImageData::D8(swash_image.data.into_iter().collect()),
                    )
                    .unwrap();

                    let mut metadata = Vec::with_capacity(8);
                    metadata.extend_from_slice(&vertex_ty.to_le_bytes());
                    metadata.extend_from_slice(&swash_image.placement.left.to_le_bytes());
                    metadata.extend_from_slice(&swash_image.placement.top.to_le_bytes());

                    let coords = self
                        .basalt
                        .atlas_ref()
                        .load_image(
                            atlas_cache_id.clone(),
                            AtlasCacheCtrl::Indefinite,
                            atlas_image,
                            metadata,
                        )
                        .unwrap();

                    atlas_coords.insert(atlas_cache_id, coords);
                }
            }

            // -- Finalize Placement -- //

            // Last line was not visible, estimate
            if max_line_y.is_none() {
                max_line_y = Some((num_lines as f32 * line_height) - (line_height - text_height));
            }

            let min_line_y = min_line_y.unwrap();
            let max_line_y = max_line_y.unwrap();
            let text_body_height = max_line_y - min_line_y;

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
            let mut glyph_vertex_data = HashMap::new();

            let text_body_min_x = bps.tli[0] + pad_l;
            let text_body_max_x = bps.tri[0] - pad_r;
            let text_body_min_y = bps.tli[1] + pad_t;
            let text_body_max_y = bps.bli[1] - pad_b;

            for (atlas_cache_id, mut glyph_x, mut glyph_y) in glyph_info {
                let coords = match atlas_coords.get(&atlas_cache_id) {
                    Some(coords) => coords.clone(),
                    None => continue,
                };

                let vertex_ty = i32::from_le_bytes(coords.metadata()[0..4].try_into().unwrap());
                let placement_left =
                    i32::from_le_bytes(coords.metadata()[4..8].try_into().unwrap());
                let placement_top =
                    i32::from_le_bytes(coords.metadata()[8..12].try_into().unwrap());
                glyph_y += vert_align_offset - placement_top as f32;
                glyph_x += placement_left as f32;

                let [glyph_w, glyph_h] = coords.width_height();
                let mut min_x = (glyph_x / context.scale) + pad_l + bps.tli[0];
                let mut min_y = (glyph_y / context.scale) + pad_t + bps.tli[1];
                let mut max_x = min_x + (glyph_w / context.scale);
                let mut max_y = min_y + (glyph_h / context.scale);
                let [mut c_min_x, mut c_min_y] = coords.top_left();
                let [mut c_max_x, mut c_max_y] = coords.bottom_right();

                if style.overflow_x != Some(true) {
                    if min_x < text_body_min_x {
                        if max_x < text_body_min_x {
                            continue;
                        }

                        let of_x = text_body_min_x - min_x;
                        min_x += of_x;
                        c_min_x += of_x;
                    }

                    if max_x > text_body_max_x {
                        if min_x > text_body_max_x {
                            continue;
                        }

                        let of_x = max_x - text_body_max_x;
                        max_x -= of_x;
                        c_max_x -= of_x;
                    }
                }

                if style.overflow_y != Some(true) {
                    if min_y < text_body_min_y {
                        if max_y < text_body_min_y {
                            break;
                        }

                        let of_y = text_body_min_y - min_y;
                        min_y += of_y;
                        c_min_y += min_y;
                    }

                    if max_y > text_body_max_y {
                        if min_y > text_body_max_y {
                            break;
                        }

                        let of_y = max_y - text_body_max_y;
                        max_y -= of_y;
                        c_max_y -= of_y;
                    }
                }

                // -- Vertex Generation -- //

                let tex_i = coords.image_id() as u32;

                glyph_vertex_data
                    .entry(tex_i)
                    .or_insert_with(Vec::new)
                    .append(&mut vec![
                        ItfVertInfo {
                            position: [max_x, min_y, content_z],
                            coords: [c_max_x, c_min_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i,
                        },
                        ItfVertInfo {
                            position: [min_x, min_y, content_z],
                            coords: [c_min_x, c_min_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i,
                        },
                        ItfVertInfo {
                            position: [min_x, max_y, content_z],
                            coords: [c_min_x, c_max_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i,
                        },
                        ItfVertInfo {
                            position: [max_x, min_y, content_z],
                            coords: [c_max_x, c_min_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i,
                        },
                        ItfVertInfo {
                            position: [min_x, max_y, content_z],
                            coords: [c_min_x, c_max_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i: 0,
                        },
                        ItfVertInfo {
                            position: [max_x, max_y, content_z],
                            coords: [c_max_x, c_max_y],
                            color: color.as_array(),
                            ty: vertex_ty,
                            tex_i,
                        },
                    ]);
            }

            for (tex_i, vertexes) in glyph_vertex_data.clone() {
                vert_data.push((vertexes, None, tex_i as u64));
            }

            bps.text_state = Some(TextState {
                atlas_coords: atlas_coords.into_values().collect(),
                style: text_style,
                text: style.text.clone(),
                body_from_t,
                body_from_l,
                vertex_data: glyph_vertex_data,
            });
        }

        if update_stats {
            stats.t_text = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // -- Get current content height before overflow checks ----------------------------- //

        for (verts, ..) in &mut vert_data {
            for vert in verts {
                if vert.position[1] < bps.unbound_mm_y[0] {
                    bps.unbound_mm_y[0] = vert.position[1];
                }

                if vert.position[1] > bps.unbound_mm_y[1] {
                    bps.unbound_mm_y[1] = vert.position[1];
                }

                if vert.position[0] < bps.unbound_mm_x[0] {
                    bps.unbound_mm_x[0] = vert.position[0];
                }

                if vert.position[0] > bps.unbound_mm_x[1] {
                    bps.unbound_mm_x[1] = vert.position[0];
                }
            }
        }

        // -- Make sure that the verts are within the boundries of all ancestors. ------ //

        let mut cut_amt;
        let mut cut_percent;
        let mut pos_min;
        let mut pos_max;
        let mut coords_min;
        let mut coords_max;
        let mut tri_dim;
        let mut img_dim;

        for (_check_bin, check_style, check_pft, check_pfl, check_w, check_h) in
            ancestor_data.iter()
        {
            let scroll_y = check_style.scroll_y.unwrap_or(0.0);
            let scroll_x = check_style.scroll_x.unwrap_or(0.0);
            let overflow_y = check_style.overflow_y.unwrap_or(false);
            let overflow_x = check_style.overflow_x.unwrap_or(false);
            let check_b = *check_pft + *check_h;
            let check_r = *check_pfl + *check_w;

            if !overflow_y {
                let bps_check_y: Vec<&mut f32> = vec![
                    &mut bps.tli[1],
                    &mut bps.tri[1],
                    &mut bps.bli[1],
                    &mut bps.bri[1],
                    &mut bps.tlo[1],
                    &mut bps.tro[1],
                    &mut bps.blo[1],
                    &mut bps.bro[1],
                ];

                for y in bps_check_y {
                    *y -= scroll_y;

                    if *y < *check_pft {
                        *y = *check_pft;
                    } else if *y > check_b {
                        *y = check_b;
                    }
                }
            }

            if !overflow_x {
                for x in [
                    &mut bps.tli[0],
                    &mut bps.tri[0],
                    &mut bps.bli[0],
                    &mut bps.bri[0],
                    &mut bps.tlo[0],
                    &mut bps.tro[0],
                    &mut bps.blo[0],
                    &mut bps.bro[0],
                ]
                .into_iter()
                {
                    *x -= scroll_x;

                    if *x < *check_pfl {
                        *x = *check_pfl;
                    } else if *x > check_r {
                        *x = check_r;
                    }
                }
            }

            for (verts, ..) in &mut vert_data {
                let mut rm_tris: Vec<usize> = Vec::new();

                for (tri_i, tri) in verts.chunks_mut(3).enumerate() {
                    tri[0].position[1] -= scroll_y;
                    tri[1].position[1] -= scroll_y;
                    tri[2].position[1] -= scroll_y;
                    tri[0].position[0] -= scroll_x;
                    tri[1].position[0] -= scroll_x;
                    tri[2].position[0] -= scroll_x;

                    if !overflow_y {
                        if (tri[0].position[1] < *check_pft
                            && tri[1].position[1] < *check_pft
                            && tri[2].position[1] < *check_pft)
                            || (tri[0].position[1] > check_b
                                && tri[1].position[1] > check_b
                                && tri[2].position[1] > check_b)
                        {
                            rm_tris.push(tri_i);
                            continue;
                        } else {
                            pos_min = tri[0].position[1]
                                .min(tri[1].position[1])
                                .min(tri[2].position[1]);
                            pos_max = tri[0].position[1]
                                .max(tri[1].position[1])
                                .max(tri[2].position[1]);
                            coords_min =
                                tri[0].coords[1].min(tri[1].coords[1]).min(tri[2].coords[1]);
                            coords_max =
                                tri[0].coords[1].max(tri[1].coords[1]).max(tri[2].coords[1]);

                            tri_dim = pos_max - pos_min;
                            img_dim = coords_max - coords_min;

                            for vert in tri.iter_mut() {
                                if vert.position[1] < *check_pft {
                                    cut_amt = check_pft - vert.position[1];
                                    cut_percent = cut_amt / tri_dim;
                                    vert.coords[1] += cut_percent * img_dim;
                                    vert.position[1] += cut_amt;
                                } else if vert.position[1] > check_b {
                                    cut_amt = vert.position[1] - check_b;
                                    cut_percent = cut_amt / tri_dim;
                                    vert.coords[1] -= cut_percent * img_dim;
                                    vert.position[1] -= cut_amt;
                                }
                            }
                        }
                    }

                    if !overflow_x {
                        if (tri[0].position[0] < *check_pfl
                            && tri[1].position[0] < *check_pfl
                            && tri[2].position[0] < *check_pfl)
                            || (tri[0].position[0] > check_r
                                && tri[1].position[0] > check_r
                                && tri[2].position[0] > check_r)
                        {
                            rm_tris.push(tri_i);
                        } else {
                            pos_min = tri[0].position[0]
                                .min(tri[1].position[0])
                                .min(tri[2].position[0]);
                            pos_max = tri[0].position[0]
                                .max(tri[1].position[0])
                                .max(tri[2].position[0]);
                            coords_min =
                                tri[0].coords[0].min(tri[1].coords[0]).min(tri[2].coords[0]);
                            coords_max =
                                tri[0].coords[0].max(tri[1].coords[0]).max(tri[2].coords[0]);

                            tri_dim = pos_max - pos_min;
                            img_dim = coords_max - coords_min;

                            for vert in tri.iter_mut() {
                                if vert.position[0] < *check_pfl {
                                    cut_amt = check_pfl - vert.position[0];
                                    cut_percent = cut_amt / tri_dim;
                                    vert.coords[0] += cut_percent * img_dim;
                                    vert.position[0] += cut_amt;
                                } else if vert.position[0] > check_r {
                                    cut_amt = vert.position[0] - check_r;
                                    cut_percent = cut_amt / tri_dim;
                                    vert.coords[0] -= cut_percent * img_dim;
                                    vert.position[0] -= cut_amt;
                                }
                            }
                        }
                    }
                }

                for tri_i in rm_tris.into_iter().rev() {
                    for i in (0..3).rev() {
                        verts.swap_remove((tri_i * 3) + i);
                    }
                }
            }
        }

        if update_stats {
            stats.t_overflow = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        // ----------------------------------------------------------------------------- //

        for &mut (ref mut verts, ..) in &mut vert_data {
            scale_verts(&context.extent, context.scale, verts);
        }

        if update_stats {
            stats.t_scale = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        *self.verts.lock() = VertexState {
            verts: vert_data,
            atlas_coords_in_use,
        };

        *self.post_update.write() = bps.clone();
        *self.last_update.lock() = Instant::now();

        if update_stats {
            stats.t_locks = inst.elapsed();
            stats.t_total += inst.elapsed();
            inst = Instant::now();
        }

        let mut internal_hooks = self.internal_hooks.lock();

        for hook_enum in internal_hooks
            .get_mut(&InternalHookTy::Updated)
            .unwrap()
            .iter_mut()
        {
            if let InternalHookFn::Updated(func) = hook_enum {
                func(self, &bps);
            }
        }

        for hook_enum in internal_hooks
            .get_mut(&InternalHookTy::UpdatedOnce)
            .unwrap()
            .drain(..)
        {
            if let InternalHookFn::Updated(mut func) = hook_enum {
                func(self, &bps);
            }
        }

        if update_stats {
            stats.t_callbacks = inst.elapsed();
            stats.t_total += inst.elapsed();
        } else {
            stats.t_total = inst.elapsed();
        }

        *self.update_stats.lock() = stats;
    }

    pub fn force_update(&self) {
        self.update.store(true, atomic::Ordering::SeqCst);
        self.basalt.interface_ref().composer_ref().unpark();
    }

    pub fn force_recursive_update(self: &Arc<Self>) {
        self.force_update();
        self.children_recursive()
            .into_iter()
            .for_each(|child| child.force_update());
    }

    pub fn update_children(&self) {
        self.update_children_priv(false);
    }

    fn update_children_priv(&self, update_self: bool) {
        if update_self {
            self.update.store(true, atomic::Ordering::SeqCst);
            self.basalt.interface_ref().composer_ref().unpark();
        }

        for child in self.children().into_iter() {
            child.update_children_priv(true);
        }
    }

    pub fn style(&self) -> Arc<BinStyle> {
        self.style.load().clone()
    }

    pub fn style_copy(&self) -> BinStyle {
        self.style.load().as_ref().clone()
    }

    #[track_caller]
    pub fn style_update(&self, copy: BinStyle) -> BinStyleValidation {
        let validation = copy.validate(self.hrchy.load().parent.is_some());

        if !validation.errors_present() {
            self.style.store(Arc::new(copy));
            *self.initial.lock() = false;
            self.update.store(true, atomic::Ordering::SeqCst);
            self.basalt.interface_ref().composer_ref().unpark();
        }

        validation
    }

    pub fn hidden(self: &Arc<Self>, to: Option<bool>) {
        let mut copy = self.style_copy();
        copy.hidden = to;
        self.style_update(copy).expect_valid();
        self.update_children();
    }
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
