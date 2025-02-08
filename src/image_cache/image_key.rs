use std::any::{Any, TypeId};
use std::hash::{BuildHasher, Hash, Hasher};
use std::mem::swap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hashbrown::hash_table::{Entry, HashTable};
use url::Url;

use crate::image_cache::GlyphCacheKey;

mod vk {
    pub use vulkano::image::Image;
    pub use vulkano_taskgraph::Id;
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum KeyKind {
    ImageCacheUrl,
    ImageCachePath,
    ImageCacheGlyph,
    ImageCacheUser(TypeId),
    VulkanoId,
    None,
}

#[derive(Clone)]
enum KeyInner {
    Shared(Arc<dyn Any + Send + Sync>),
    VulkanoId(vk::Id<vk::Image>),
    None,
}

/// `ImageKey` is a value used to reference an image.
#[derive(Clone)]
pub struct ImageKey {
    hash: u64,
    kind: KeyKind,
    inner: KeyInner,
}

impl ImageKey {
    pub(crate) const NONE: Self = Self {
        hash: 0,
        kind: KeyKind::None,
        inner: KeyInner::None,
    };

    /// Creates an `ImageKey` from the provided URL. This will not load the image.
    pub fn url<U: AsRef<str>>(url: U) -> Result<Self, String> {
        Url::parse(url.as_ref())
            .map_err(|e| format!("Invalid URL: {}", e))
            .map(Self::from)
    }

    /// Returns `true` if this cache key is a url.
    pub fn is_url(&self) -> bool {
        matches!(self.kind, KeyKind::ImageCacheUrl)
    }

    /// Returns a reference to `Url` if the cache key is a url.
    pub fn as_url(&self) -> Option<&Url> {
        if self.is_url() {
            match &self.inner {
                KeyInner::Shared(inner) => Some(inner.downcast_ref::<Url>().unwrap()),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    /// Create an `ImageKey` from the provided path. This will not load the image.
    pub fn path<P: AsRef<Path>>(path: P) -> Self {
        Self::from(path.as_ref().to_path_buf())
    }

    /// Returns `true` if this cache key is a path.
    pub fn is_path(&self) -> bool {
        matches!(self.kind, KeyKind::ImageCachePath)
    }

    /// Returns a reference to `PathBuf` if the cache key is a path.
    pub fn as_path(&self) -> Option<&PathBuf> {
        if self.is_path() {
            match &self.inner {
                KeyInner::Shared(inner) => Some(inner.downcast_ref::<PathBuf>().unwrap()),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    /// Create an `ImageKey` from the user provided key.
    pub fn user<T>(key: T) -> Self
    where
        T: Any + Hash + Send + Sync,
    {
        let kind = KeyKind::ImageCacheUser(key.type_id());
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        kind.hash(&mut hasher);
        key.hash(&mut hasher);

        Self {
            hash: hasher.finish(),
            kind,
            inner: KeyInner::Shared(Arc::new(key)),
        }
    }

    /// Returns `true` if this cache key is the provided user key type.
    pub fn is_user<T>(&self) -> bool
    where
        T: Any,
    {
        match self.kind {
            KeyKind::ImageCacheUser(type_id) => type_id == TypeId::of::<T>(),
            _ => false,
        }
    }

    /// Returns `true` if this cache key is any user key.
    pub fn is_any_user(&self) -> bool {
        matches!(self.kind, KeyKind::ImageCacheUser(..))
    }

    /// Returns a reference to `T` if the cache key is `T`.
    pub fn as_user<T>(&self) -> Option<&T>
    where
        T: Any,
    {
        if self.is_any_user() {
            match &self.inner {
                KeyInner::Shared(inner) => Some(inner.downcast_ref::<T>().unwrap()),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    pub(crate) fn glyph(cache_key: GlyphCacheKey) -> Self {
        let kind = KeyKind::ImageCacheGlyph;
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        kind.hash(&mut hasher);
        cache_key.hash(&mut hasher);

        Self {
            hash: hasher.finish(),
            kind,
            inner: KeyInner::Shared(Arc::new(cache_key)),
        }
    }

    pub(crate) fn is_glyph(&self) -> bool {
        matches!(self.kind, KeyKind::ImageCacheGlyph)
    }

    pub(crate) fn as_glyph(&self) -> Option<&GlyphCacheKey> {
        if self.is_glyph() {
            match &self.inner {
                KeyInner::Shared(inner) => Some(inner.downcast_ref::<GlyphCacheKey>().unwrap()),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    pub(crate) fn vulkano_id(id: vk::Id<vk::Image>) -> Self {
        let kind = KeyKind::VulkanoId;
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        kind.hash(&mut hasher);
        id.hash(&mut hasher);

        Self {
            hash: hasher.finish(),
            kind,
            inner: KeyInner::VulkanoId(id),
        }
    }

    pub(crate) fn is_vulkano_id(&self) -> bool {
        matches!(self.kind, KeyKind::VulkanoId)
    }

    pub(crate) fn as_vulkano_id(&self) -> Option<vk::Id<vk::Image>> {
        if self.is_vulkano_id() {
            match self.inner {
                KeyInner::VulkanoId(id) => Some(id),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    pub(crate) fn is_none(&self) -> bool {
        matches!(self.kind, KeyKind::None)
    }

    pub(crate) fn is_image_cache(&self) -> bool {
        matches!(
            self.kind,
            KeyKind::ImageCacheUrl
                | KeyKind::ImageCachePath
                | KeyKind::ImageCacheGlyph
                | KeyKind::ImageCacheUser(..)
        )
    }
}

impl From<Url> for ImageKey {
    fn from(url: Url) -> Self {
        let kind = KeyKind::ImageCacheUrl;
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        kind.hash(&mut hasher);
        url.hash(&mut hasher);

        Self {
            hash: hasher.finish(),
            kind,
            inner: KeyInner::Shared(Arc::new(url)),
        }
    }
}

impl From<&Path> for ImageKey {
    fn from(path: &Path) -> Self {
        Self::path(path)
    }
}

impl From<PathBuf> for ImageKey {
    fn from(path: PathBuf) -> Self {
        let kind = KeyKind::ImageCachePath;
        let mut hasher = foldhash::fast::FixedState::with_seed(0).build_hasher();
        kind.hash(&mut hasher);
        path.hash(&mut hasher);

        Self {
            hash: hasher.finish(),
            kind,
            inner: KeyInner::Shared(Arc::new(path)),
        }
    }
}

impl std::fmt::Debug for ImageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            KeyKind::ImageCacheUrl => {
                f.debug_tuple("ImageKey::ImageCacheUrl")
                    .field(self.as_url().unwrap())
                    .finish()
            },
            KeyKind::ImageCachePath => {
                f.debug_tuple("ImageKey::ImageCachePath")
                    .field(self.as_path().unwrap())
                    .finish()
            },
            KeyKind::ImageCacheGlyph => {
                f.debug_tuple("ImageKey::ImageCacheGlyph")
                    .field(self.as_glyph().unwrap())
                    .finish()
            },
            KeyKind::ImageCacheUser(..) => {
                f.debug_tuple("ImageKey::ImageCacheUser")
                    .finish_non_exhaustive()
            },
            KeyKind::VulkanoId => {
                f.debug_tuple("ImageKey::VulkanoId")
                    .field(&self.as_vulkano_id().unwrap())
                    .finish()
            },
            KeyKind::None => f.debug_struct("ImageKey::None").finish(),
        }
    }
}

impl Hash for ImageKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl PartialEq for ImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.hash == other.hash
    }
}

impl Eq for ImageKey {}

#[derive(Clone)]
struct KeyVal<V> {
    key: ImageKey,
    val: V,
}

#[derive(Clone)]
pub struct ImageMap<V> {
    inner: HashTable<KeyVal<V>>,
}

impl<V> ImageMap<V> {
    pub fn new() -> Self {
        Self {
            inner: HashTable::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashTable::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn retain(&mut self, mut retain: impl FnMut(&ImageKey, &mut V) -> bool) {
        self.inner.retain(|kv| retain(&kv.key, &mut kv.val));
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional, |kv| kv.key.hash);
    }

    pub fn contains(&self, key: &ImageKey) -> bool {
        self.inner.find(key.hash, |kv| kv.key == *key).is_some()
    }

    pub fn get(&self, key: &ImageKey) -> Option<&V> {
        self.inner
            .find(key.hash, |kv| kv.key == *key)
            .map(|kv| &kv.val)
    }

    pub fn get_mut(&mut self, key: &ImageKey) -> Option<&mut V> {
        self.inner
            .find_mut(key.hash, |kv| kv.key == *key)
            .map(|kv| &mut kv.val)
    }

    pub fn insert(&mut self, key: ImageKey, mut val: V) -> Option<V> {
        match self
            .inner
            .entry(key.hash, |kv| kv.key == key, |kv| kv.key.hash)
        {
            Entry::Occupied(mut entry) => {
                swap(&mut entry.get_mut().val, &mut val);
                Some(val)
            },
            Entry::Vacant(entry) => {
                entry.insert(KeyVal {
                    key,
                    val,
                });
                None
            },
        }
    }

    pub fn try_insert(&mut self, key: &ImageKey, insert: impl FnOnce() -> V) -> bool {
        match self
            .inner
            .entry(key.hash, |kv| kv.key == *key, |kv| kv.key.hash)
        {
            Entry::Occupied(..) => false,
            Entry::Vacant(entry) => {
                entry.insert(KeyVal {
                    key: key.clone(),
                    val: insert(),
                });
                true
            },
        }
    }

    pub fn set(&mut self, key: ImageKey, val: V) {
        match self
            .inner
            .entry(key.hash, |kv| kv.key == key, |kv| kv.key.hash)
        {
            Entry::Occupied(mut entry) => {
                entry.get_mut().val = val;
            },
            Entry::Vacant(entry) => {
                entry.insert(KeyVal {
                    key,
                    val,
                });
            },
        }
    }

    pub fn modify(
        &mut self,
        key: &ImageKey,
        insert: impl FnOnce() -> V,
        modify: impl FnOnce(&mut V),
    ) {
        match self
            .inner
            .entry(key.hash, |kv| kv.key == *key, |kv| kv.key.hash)
        {
            Entry::Occupied(mut entry) => {
                modify(&mut entry.get_mut().val);
            },
            Entry::Vacant(entry) => {
                let mut val = insert();
                modify(&mut val);

                entry.insert(KeyVal {
                    key: key.clone(),
                    val,
                });
            },
        }
    }

    pub fn remove(&mut self, key: &ImageKey) -> Option<V> {
        match self.inner.find_entry(key.hash, |kv| kv.key == *key) {
            Ok(entry) => Some(entry.remove().0.val),
            Err(..) => None,
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (ImageKey, V)> + use<'_, V> {
        self.inner.drain().map(|kv| (kv.key, kv.val))
    }

    pub fn keys(&self) -> impl Iterator<Item = &ImageKey> {
        self.inner.iter().map(|kv| &kv.key)
    }

    pub fn into_keys(self) -> impl Iterator<Item = ImageKey> {
        self.inner.into_iter().map(|kv| kv.key)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.inner.iter().map(|kv| &kv.val)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.inner.iter_mut().map(|kv| &mut kv.val)
    }

    pub fn into_values(self) -> impl Iterator<Item = V> {
        self.inner.into_iter().map(|kv| kv.val)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ImageKey, &V)> {
        self.inner.iter().map(|kv| (&kv.key, &kv.val))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ImageKey, &mut V)> {
        self.inner.iter_mut().map(|kv| (&kv.key, &mut kv.val))
    }
}

impl<V> Extend<(ImageKey, V)> for ImageMap<V> {
    fn extend<I: IntoIterator<Item = (ImageKey, V)>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.reserve(iter.size_hint().0);
        iter.for_each(|(k, v)| self.set(k, v));
    }
}

impl<V> FromIterator<(ImageKey, V)> for ImageMap<V> {
    fn from_iter<I: IntoIterator<Item = (ImageKey, V)>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let mut map = Self::with_capacity(iter.size_hint().0);
        map.extend(iter);
        map
    }
}

pub struct ImageMapIntoIterator<V>(hashbrown::hash_table::IntoIter<KeyVal<V>>);

impl<V> Iterator for ImageMapIntoIterator<V> {
    type Item = (ImageKey, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|kv| (kv.key, kv.val))
    }
}

impl<V> IntoIterator for ImageMap<V> {
    type IntoIter = ImageMapIntoIterator<V>;
    type Item = (ImageKey, V);

    fn into_iter(self) -> Self::IntoIter {
        ImageMapIntoIterator(self.inner.into_iter())
    }
}

impl<V> std::fmt::Debug for ImageMap<V>
where
    V: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_map = f.debug_map();

        for (image_key, val) in self.iter() {
            debug_map.entry(image_key, val);
        }

        debug_map.finish()
    }
}

#[derive(Clone)]
pub struct ImageSet {
    inner: HashTable<ImageKey>,
}

impl ImageSet {
    pub fn new() -> Self {
        Self {
            inner: HashTable::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: HashTable::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn retain(&mut self, mut retain: impl FnMut(&ImageKey) -> bool) {
        self.inner.retain(|key| retain(key));
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional, |key| key.hash);
    }

    pub fn contains(&self, key: &ImageKey) -> bool {
        self.inner.find(key.hash, |k| *k == *key).is_some()
    }

    pub fn insert(&mut self, key: ImageKey) -> bool {
        match self.inner.entry(key.hash, |k| *k == key, |k| k.hash) {
            Entry::Occupied(..) => false,
            Entry::Vacant(entry) => {
                entry.insert(key);
                true
            },
        }
    }

    pub fn remove(&mut self, key: &ImageKey) -> bool {
        match self.inner.find_entry(key.hash, |k| *k == *key) {
            Ok(entry) => {
                entry.remove();
                true
            },
            Err(..) => false,
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = ImageKey> + use<'_> {
        self.inner.drain()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ImageKey> {
        self.inner.iter()
    }
}

impl Extend<ImageKey> for ImageSet {
    fn extend<I: IntoIterator<Item = ImageKey>>(&mut self, iter: I) {
        let iter = iter.into_iter();
        self.reserve(iter.size_hint().0);
        iter.for_each(|k| {
            self.insert(k);
        });
    }
}

impl FromIterator<ImageKey> for ImageSet {
    fn from_iter<I: IntoIterator<Item = ImageKey>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let mut set = Self::with_capacity(iter.size_hint().0);
        set.extend(iter);
        set
    }
}

pub struct ImageSetIntoIterator(hashbrown::hash_table::IntoIter<ImageKey>);

impl Iterator for ImageSetIntoIterator {
    type Item = ImageKey;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl IntoIterator for ImageSet {
    type IntoIter = ImageSetIntoIterator;
    type Item = ImageKey;

    fn into_iter(self) -> Self::IntoIter {
        ImageSetIntoIterator(self.inner.into_iter())
    }
}

impl std::fmt::Debug for ImageSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_list = f.debug_list();

        for image_key in self.iter() {
            debug_list.entry(image_key);
        }

        debug_list.finish()
    }
}
