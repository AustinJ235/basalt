use allsorts::error::ParseError;
use allsorts::error::ShapingError;

#[derive(Clone,Debug,PartialEq)]
pub struct BstTextError {
	pub src: BstTextErrorSrc,
	pub ty: BstTextErrorTy,
}

#[derive(Clone,Debug,PartialEq)]
pub enum BstTextErrorSrc {
	Unknown,
	File,
	Cmap,
	Maxp,
	GDEF,
	GPOS,
	Hhea,
	Hmtx,
	Head,
	Loca,
	Glyf,
	Gsub,
	GsubInfo,
	Glyph,
	Bitmap,
}

#[derive(Clone,Debug,PartialEq)]
pub enum BstTextErrorTy {
	Unimplemented,
	FileGeneric,
	FileBadEof,
	FileBadValue,
	FileBadVersion,
	FileBadOffset,
	FileBadIndex,
	FileLimitExceeded,
	FileMissingValue,
	FileCompressionError,
	FileUnsupportedFormat,
	FileMissingTable,
	FileMissingSubTable,
	MissingIndex,
	MissingGlyph,
	UnimplementedDataTy,
	Other(String),
}

impl BstTextError {
	pub fn unimplemented() -> Self {
		Self::src_and_ty(
			BstTextErrorSrc::Unknown,
			BstTextErrorTy::Unimplemented
		)
	}

	pub fn src_and_ty(src: BstTextErrorSrc, ty: BstTextErrorTy) -> Self {
		BstTextError {
			src,
			ty,
		}
	}
	
	pub fn allsorts_parse(src: BstTextErrorSrc, err: ParseError) -> Self {
		BstTextError {
			src: src,
			ty: match err {
				ParseError::BadEof => BstTextErrorTy::FileBadEof,
				ParseError::BadValue => BstTextErrorTy::FileBadValue,
				ParseError::BadVersion => BstTextErrorTy::FileBadVersion,
				ParseError::BadOffset => BstTextErrorTy::FileBadOffset,
				ParseError::BadIndex => BstTextErrorTy::FileBadIndex,
				ParseError::LimitExceeded => BstTextErrorTy::FileLimitExceeded,
				ParseError::MissingValue => BstTextErrorTy::FileMissingValue,
				ParseError::CompressionError => BstTextErrorTy::FileCompressionError,
				ParseError::NotImplemented => BstTextErrorTy::FileGeneric,
			}
		}
	}
	
	// TODO: Implement mapping of ShapingError
	pub fn allsorts_shaping(src: BstTextErrorSrc, err: ShapingError) -> Self {
		
		println!("Basalt Text: Returning unimplemented error! src: {:?}, err: {:?}", src, err);
		
		Self::src_and_ty(
			src,
			BstTextErrorTy::Unimplemented
		)
	}
}
