use allsorts::binary::read::ReadScope;
use allsorts::font_data_impl::read_cmap_subtable;
use allsorts::gpos::{gpos_apply,Info};
use allsorts::gsub::{gsub_apply_default,RawGlyph,GlyphOrigin};
use allsorts::tables::cmap::Cmap;
use allsorts::tables::{MaxpTable,OpenTypeFile,OpenTypeFont};
use allsorts::layout::{new_layout_cache,GDEFTable,LayoutTable,GPOS,GSUB};
use allsorts::tag;

#[test]
fn test() {
	shape_text("Hello World!", tag::from_string("DFLT").unwrap(), tag::from_string("dflt").unwrap());
}

pub fn shape_text<T: AsRef<str>>(text: T, script: u32, lang: u32) {
	let file = ReadScope::new(include_bytes!("Alata-Regular.otf")).read::<OpenTypeFile>().unwrap();
	let scope = file.scope;
	let otf = match file.font { OpenTypeFont::Single(v) => v, _ => panic!() };	
	let cmap = otf.read_table(&scope, tag::CMAP).unwrap().map(|v| v.read::<Cmap>().unwrap()).unwrap();
	let cmap_subtable = read_cmap_subtable(&cmap).unwrap().unwrap();
	let num_glyphs = otf.read_table(&scope, tag::MAXP).unwrap().map(|v| v.read::<MaxpTable>().unwrap()).unwrap().num_glyphs;
	
	let map_char = |c| {
		RawGlyph {
			unicodes: vec![c],
			glyph_index: cmap_subtable.map_glyph(c as u32).unwrap(),
			liga_component_pos: 0,
			glyph_origin: GlyphOrigin::Char(c),
			small_caps: false,
			multi_subst_dup: false,
			is_vert_alt: false,
			fake_bold: false,
			fake_italic: false,
			extra_data: (),
		}
	};
	
	let mut glyphs: Vec<_> = text.as_ref().chars().map(|c| map_char(c)).collect();
	let gsub_table = otf.find_table_record(tag::GSUB).unwrap()
		.read_table(&scope).unwrap().read::<LayoutTable<GSUB>>().unwrap();
	let opt_gdef_table = otf.find_table_record(tag::GDEF).map(|gdef_record|
		gdef_record.read_table(&scope).unwrap().read::<GDEFTable>().unwrap());
	let opt_gpos_table = otf.find_table_record(tag::GPOS).map(|gpos_record|
		gpos_record.read_table(&scope).unwrap().read::<LayoutTable<GPOS>>().unwrap());
	let gsub_cache = new_layout_cache(gsub_table);
	let vertical = false;
	
	gsub_apply_default(
		&|| vec![map_char('\u{25cc}')],
		&gsub_cache,
		opt_gdef_table.as_ref(),
		script,
		lang,
		vertical,
		num_glyphs,
		&mut glyphs
	).unwrap();
	
	if let Some(gpos_table) = opt_gpos_table {
		let mut infos = Info::init_from_glyphs(opt_gdef_table.as_ref(), glyphs).unwrap();
		let gpos_cache = new_layout_cache(gpos_table);
		let kerning = true;
	
		gpos_apply(
			&gpos_cache,
			opt_gdef_table.as_ref(),
			kerning,
			script,
			lang,
			&mut infos
		).unwrap();
		
		infos.into_iter().for_each(|i| println!("{:?}", i));
	}
}

