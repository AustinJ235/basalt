use std::cmp::Ordering;

use crate::NonExhaustive;
use crate::interface::{
    Color, FontFamily, FontStretch, FontStyle, FontWeight, LineLimit, LineSpacing, TextHoriAlign,
    TextVertAlign, TextWrap, UnitValue,
};

/// Attributes of text.
#[derive(Debug, Clone, PartialEq)]
pub struct TextAttrs {
    pub color: Color,
    pub height: UnitValue,
    pub secret: bool,
    pub font_family: FontFamily,
    pub font_weight: FontWeight,
    pub font_stretch: FontStretch,
    pub font_style: FontStyle,
    pub _ne: NonExhaustive,
}

impl Default for TextAttrs {
    fn default() -> Self {
        Self {
            color: Color::black(),
            height: Default::default(),
            secret: false,
            font_family: Default::default(),
            font_weight: Default::default(),
            font_stretch: Default::default(),
            font_style: Default::default(),
            _ne: NonExhaustive(()),
        }
    }
}

/// A span of text within `TextBody`.
///
/// A span consist of the text and its text attributes.
///
/// The default values for `attrs` will inheirt those set in
/// [`TextBody.base_attrs`](struct.TextBody.html#structfield.base_attrs).
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub attrs: TextAttrs,
    pub _ne: NonExhaustive,
}

impl Default for TextSpan {
    fn default() -> Self {
        Self {
            text: String::new(),
            attrs: TextAttrs {
                color: Default::default(),
                ..Default::default()
            },
            _ne: NonExhaustive(()),
        }
    }
}

impl<T> From<T> for TextSpan
where
    T: Into<String>,
{
    fn from(from: T) -> Self {
        Self {
            text: from.into(),
            ..Default::default()
        }
    }
}

impl TextSpan {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// The text body of a `Bin`.
///
/// Each [`BinStyle`](`crate::interface::BinStyle`) has a single `TextBody`. It can contain multiple
/// [`TextSpan`](`TextSpan`).
///
/// The default values for `base_attrs` will inheirt those set with
/// [`Interface::set_default_font`](`crate::interface::Interface::set_default_font`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBody {
    pub spans: Vec<TextSpan>,
    pub line_spacing: LineSpacing,
    pub line_limit: LineLimit,
    pub text_wrap: TextWrap,
    pub vert_align: TextVertAlign,
    pub hori_align: TextHoriAlign,
    pub base_attrs: TextAttrs,
    pub cursor: TextCursor,
    pub cursor_color: Color,
    pub selection: Option<TextSelection>,
    pub selection_color: Color,
    pub _ne: NonExhaustive,
}

impl Default for TextBody {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            line_spacing: Default::default(),
            line_limit: Default::default(),
            text_wrap: Default::default(),
            vert_align: Default::default(),
            hori_align: Default::default(),
            base_attrs: TextAttrs::default(),
            cursor: Default::default(),
            cursor_color: Color::black(),
            selection: None,
            selection_color: Color::shex("4040ffc0"),
            _ne: NonExhaustive(()),
        }
    }
}

impl<T> From<T> for TextBody
where
    T: Into<String>,
{
    fn from(from: T) -> Self {
        Self {
            spans: vec![TextSpan::from(from)],
            ..Default::default()
        }
    }
}

/// A positioned cursor within [`TextBody`].
///
/// **Note:** May become invalid if the [`TextBody`] is modified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosTextCursor {
    pub span: usize,
    pub byte_s: usize,
    pub byte_e: usize,
    pub affinity: TextCursorAffinity,
}

/// A cursor within [`TextBody`].
///
/// **Note:** May become invalid if the [`TextBody`] is modified.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCursor {
    #[default]
    None,
    Empty,
    Position(PosTextCursor),
}

impl TextCursor {
    pub fn into_position(self) -> Option<PosTextCursor> {
        match self {
            Self::Position(cursor) => Some(cursor),
            _ => None,
        }
    }
}

impl From<PosTextCursor> for TextCursor {
    fn from(cursor: PosTextCursor) -> TextCursor {
        TextCursor::Position(cursor)
    }
}

impl PartialOrd for PosTextCursor {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PosTextCursor {
    fn cmp(&self, other: &Self) -> Ordering {
        self.span.cmp(&other.span).then(
            self.byte_s
                .cmp(&other.byte_s)
                .then(self.affinity.cmp(&other.affinity)),
        )
    }
}

/// The affinity of a text cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCursorAffinity {
    Before,
    After,
}

impl PartialOrd for TextCursorAffinity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TextCursorAffinity {
    fn cmp(&self, other: &Self) -> Ordering {
        match self {
            Self::Before => {
                match other {
                    Self::Before => Ordering::Equal,
                    Self::After => Ordering::Less,
                }
            },
            Self::After => {
                match other {
                    Self::Before => Ordering::Greater,
                    Self::After => Ordering::Equal,
                }
            },
        }
    }
}

/// A text selection with a [`TextBody`].
///
/// **Note:** May become invalid if the [`TextBody`] is modified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    pub start: PosTextCursor,
    pub end: PosTextCursor,
}

impl TextSelection {
    pub fn extend<W>(self, extend_with: W) -> Self
    where
        W: ExtendTextSelection,
    {
        extend_with.extend_selection(self).unwrap_or(self)
    }
}

/// Trait used for types that can extend a [`TextSelection`].
pub trait ExtendTextSelection {
    fn extend_selection(self, selection: TextSelection) -> Option<TextSelection>;
}

impl ExtendTextSelection for TextCursor {
    fn extend_selection(self, selection: TextSelection) -> Option<TextSelection> {
        match self {
            Self::Empty | Self::None => return None,
            Self::Position(cursor) => cursor.extend_selection(selection),
        }
    }
}

impl ExtendTextSelection for PosTextCursor {
    fn extend_selection(self, mut selection: TextSelection) -> Option<TextSelection> {
        if self < selection.start {
            selection.start = self;
        } else if self > selection.end {
            selection.end = self;
        }

        if selection.start == selection.end {
            None
        } else {
            Some(selection)
        }
    }
}

impl ExtendTextSelection for TextSelection {
    fn extend_selection(self, mut selection: TextSelection) -> Option<TextSelection> {
        if self.start < selection.start {
            selection.start = self.start;
        }

        if self.end > selection.end {
            selection.end = self.end;
        }

        if selection.start == selection.end {
            None
        } else {
            Some(selection)
        }
    }
}
