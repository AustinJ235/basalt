//! Clipboard related objects

use std::hash::{BuildHasher, Hash, Hasher};

use parking_lot::Mutex;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
/// An item that may be stored within the clipboard.
pub enum ClipboardItem {
    PlainText(String),
    // TODO: RichText & Images?
}

impl ClipboardItem {
    fn os_hash(&self) -> u64 {
        let mut hasher = foldhash::quality::FixedState::with_seed(0).build_hasher();

        match self {
            Self::PlainText(text) => text.hash(&mut hasher),
        }

        hasher.finish()
    }
}

impl From<&str> for ClipboardItem {
    fn from(from: &str) -> Self {
        Self::PlainText(from.to_string())
    }
}

impl From<String> for ClipboardItem {
    fn from(from: String) -> Self {
        Self::PlainText(from)
    }
}

impl From<&String> for ClipboardItem {
    fn from(from: &String) -> Self {
        Self::PlainText(from.clone())
    }
}

/// Object for accessing the clipboard.
///
/// **Note:** for interaction with the OS's clipboard, the `clipboard` feature msut be enabled.
pub struct Clipboard {
    state: Mutex<State>,
}

struct State {
    #[cfg(feature = "clipboard")]
    os_clipboard: Option<arboard::Clipboard>,
    stored_item: Option<StoredClipboardItem>,
}

struct StoredClipboardItem {
    os_hash: Option<u64>,
    inner: ClipboardItem,
}

impl Clipboard {
    pub(crate) fn new() -> Self {
        #[cfg(feature = "clipboard")]
        {
            Self {
                state: Mutex::new(State {
                    os_clipboard: arboard::Clipboard::new().ok(),
                    stored_item: None,
                }),
            }
        }
        #[cfg(not(feature = "clipboard"))]
        {
            Self {
                state: Mutex::new(State {
                    stored_item: None,
                }),
            }
        }
    }

    /// Obtain the value within the clipboard.
    pub fn get(&self) -> Option<ClipboardItem> {
        let mut state = self.state.lock();

        #[cfg(feature = "clipboard")]
        {
            // TODO: As ClipboardItem gets more variants support other types of clipboard data.

            // NOTE: This attempts to get the os's clipboard and compares into the hash of the
            //       stored clipboard item. The hash is of how that item would be stored on the os.
            //       If the hash is the same, returns the stored value instead. This allows for
            //       basalt specific types in the future, that may be applicable for the os
            //       clipboard, but not the same as what would be created from the os clipboard.

            if let Some(os_clipboard) = state.os_clipboard.as_mut() {
                if let Ok(text) = os_clipboard.get_text() {
                    if let Some(stored_item) = state.stored_item.as_ref() {
                        if let Some(stored_os_hash) = stored_item.os_hash {
                            let mut hasher =
                                foldhash::quality::FixedState::with_seed(0).build_hasher();
                            text.hash(&mut hasher);
                            let os_hash = hasher.finish();

                            if os_hash == stored_os_hash {
                                return Some(stored_item.inner.clone());
                            }
                        }
                    }

                    state.stored_item = None;
                    return Some(ClipboardItem::PlainText(text));
                }
            }
        }

        state.stored_item.as_ref().map(|item| item.inner.clone())
    }

    /// Obtain the value within the clipboard.
    pub fn set<I>(&self, item: I)
    where
        I: Into<ClipboardItem>,
    {
        let item = item.into();
        let mut state = self.state.lock();

        let os_hash = {
            #[cfg(feature = "clipboard")]
            {
                match state.os_clipboard.as_mut() {
                    Some(os_clipboard) => {
                        match &item {
                            ClipboardItem::PlainText(text) => {
                                match os_clipboard.set_text(text.clone()) {
                                    Ok(_) => Some(item.os_hash()),
                                    Err(_) => None,
                                }
                            },
                        }
                    },
                    None => None,
                }
            }
            #[cfg(not(feature = "clipboard"))]
            {
                None
            }
        };

        state.stored_item = Some(StoredClipboardItem {
            os_hash,
            inner: item,
        });
    }
}
