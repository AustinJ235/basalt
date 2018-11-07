use harfbuzz_sys;
use freetype;
use interface::interface::ItfVertInfo;

pub(crate) fn render_text<T: AsRef<str>>(text: T, size: u32) -> Result<Vec<ItfVertInfo>, String> {
	unimplemented!()
}
