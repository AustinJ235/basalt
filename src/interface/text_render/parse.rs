use allsorts::binary::read::ReadScope;
use allsorts::font_data_impl::read_cmap_subtable;
use allsorts::gpos::{gpos_apply,Info,Placement};
use allsorts::gsub::{RawGlyph,GlyphOrigin};
use allsorts::tables::cmap::Cmap;
use allsorts::tables::{MaxpTable,OpenTypeFile,OpenTypeFont};
use allsorts::layout::{new_layout_cache,GDEFTable,LayoutTable,GPOS,GSUB};
use allsorts::tag;
use allsorts::tables::HmtxTable;
use allsorts::tables::HheaTable;
use allsorts::tables::glyf::GlyfTable;
use allsorts::tables::loca::LocaTable;
use allsorts::tables::HeadTable;
use allsorts::tables::glyf::{self,GlyfRecord};
use allsorts::gsub::gsub_apply_default;
use std::sync::Arc;
use std::collections::BTreeMap;

pub use super::font::{BstFont,BstFontWeight};
pub use super::glyph::{BstGlyph,BstGlyphRaw,BstGlyphPos,BstGlyphPoint,BstGlyphGeo};
pub use super::error::{BstTextError,BstTextErrorSrc,BstTextErrorTy};
pub use super::script::{BstTextScript,BstTextLang};

const ABEEZEE_REGULAR_BYTES: &'static [u8] = include_bytes!("../ABeeZee-Regular.ttf");

pub fn parse_and_shape<T: AsRef<str>>(text: T, script: BstTextScript, lang: BstTextLang) -> Result<Vec<BstGlyph>, BstTextError> {
	let file = ReadScope::new(ABEEZEE_REGULAR_BYTES)
		.read::<OpenTypeFile>()
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::File, e))?;
		
	let scope = file.scope;
	let otf = match file.font {
		OpenTypeFont::Single(v) => v,
		_ => return Err(BstTextError::src_and_ty(BstTextErrorSrc::File, BstTextErrorTy::FileUnsupportedFormat))
	};	
		
	let cmap = otf.find_table_record(tag::CMAP)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Cmap, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Cmap, e))?
		.read::<Cmap>()
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Cmap, e))?;
	
	let cmap_subtable = read_cmap_subtable(&cmap)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Cmap, e))?
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Cmap, BstTextErrorTy::FileMissingSubTable))?;
		
	let maxp = otf.find_table_record(tag::MAXP)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Maxp, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Maxp, e))?
		.read::<MaxpTable>()
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Maxp, e))?;
		
	let opt_gdef_table = match otf.find_table_record(tag::GDEF) {
		None => None,
		Some(v) => Some(v.read_table(&scope)
			.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GDEF, e))?
			.read::<GDEFTable>()
			.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GDEF, e))?)
	};
	
	let opt_gpos_table = match otf.find_table_record(tag::GPOS) {
		None => None,
		Some(v) => Some(v.read_table(&scope)
			.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GPOS, e))?
			.read::<LayoutTable<GPOS>>()
			.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GPOS, e))?)
	};
	
	let hhea = otf.find_table_record(tag::HHEA)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Hhea, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Hhea, e))?
		.read::<HheaTable>()
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Hhea, e))?;
	
	let hmtx = otf.find_table_record(tag::HMTX)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Hmtx, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Hmtx, e))?
		.read_dep::<HmtxTable>((maxp.num_glyphs as usize, hhea.num_h_metrics as usize))
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Hmtx, e))?;
	
	let head = otf.find_table_record(tag::HEAD)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Head, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Head, e))?
		.read::<HeadTable>()
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Head, e))?;
	
	let loca = otf.find_table_record(tag::LOCA)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Loca, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Loca, e))?
		.read_dep::<LocaTable>((maxp.num_glyphs as usize, head.index_to_loc_format))
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Loca, e))?;
	
	let mut glyf = otf.find_table_record(tag::GLYF)
		.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Glyf, BstTextErrorTy::FileMissingTable))?
		.read_table(&scope)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Glyf, e))?
		.read_dep::<GlyfTable>(&loca)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Glyf, e))?;
	
	let opt_gsub_table = match otf.find_table_record(tag::GSUB) {
		None => None,
		Some(v) => Some(v.read_table(&scope)
			.map_err(|e|  BstTextError::allsorts_parse(BstTextErrorSrc::Gsub, e))?
			.read::<LayoutTable<GSUB>>()
			.map_err(|e|  BstTextError::allsorts_parse(BstTextErrorSrc::Gsub, e))?)
	};
	
	let default_dpi = 72.0;
	let default_pixel_height = 30.0;
	let scaler = ((default_pixel_height * 1.33) * default_dpi) / (default_dpi * head.units_per_em as f32);
	
	let bst_font = Arc::new(BstFont {
		name: String::from("ABeeZee"),
		weight: BstFontWeight::Regular,
		default_dpi,
		default_pixel_height,
	});
			
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
	
	if let Some(gsub_table) = opt_gsub_table {
		let gsub_cache = new_layout_cache(gsub_table);
		
		gsub_apply_default(
			&|| vec![map_char('\u{25cc}')],
			&gsub_cache,
			opt_gdef_table.as_ref(),
			script.tag(),
			lang.tag(),
			false,
			maxp.num_glyphs,
			&mut glyphs
		).map_err(|e| BstTextError::allsorts_shaping(BstTextErrorSrc::Gsub, e))?;
	}
	
	let mut infos = Info::init_from_glyphs(opt_gdef_table.as_ref(), glyphs)
		.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GsubInfo, e))?;
	
	if let Some(gpos_table) = opt_gpos_table {
		let gpos_cache = new_layout_cache(gpos_table);
		let kerning = true;
	
		gpos_apply(
			&gpos_cache,
			opt_gdef_table.as_ref(),
			kerning,
			script.tag(),
			lang.tag(),
			&mut infos
		).map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::GPOS, e))?;
	}
	
	let mut x: f32 = 0.0;
	let y: f32 = 0.0;
	let mut bst_glyphs = Vec::new();
	let mut bst_glyphs_raw: BTreeMap<u16, Arc<BstGlyphRaw>> = BTreeMap::new();
	
	for info in infos {
		let glyph_index = info.glyph.glyph_index
			.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Glyph, BstTextErrorTy::MissingIndex))?;
		let hori_adv = hmtx.horizontal_advance(glyph_index, hhea.num_h_metrics)
			.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Glyph, e))? as f32 * scaler;
		
		let (placement_x, placement_y) = match info.placement {
			Placement::Distance(dist_x, dist_y) => {
				let dist_x = dist_x as f32 * scaler;
				let dist_y = dist_y as f32 * scaler;
				(x + dist_x, y + dist_y)
			},
			Placement::Anchor(_, _) | Placement::None => {
				(x, y)
			}
		};
		
		x += hori_adv;
		
		let glyf_record = glyf.records.get_mut(glyph_index as usize)
			.ok_or(BstTextError::src_and_ty(BstTextErrorSrc::Glyf, BstTextErrorTy::MissingGlyph))?;
		
		if let Some(parsed_record) = match &glyf_record {
			&GlyfRecord::Present(ref record_scope) => Some(
				GlyfRecord::Parsed(
					record_scope.read::<glyf::Glyph>()
						.map_err(|e| BstTextError::allsorts_parse(BstTextErrorSrc::Glyf, e))?
				)
			), _ => None
		} {
			*glyf_record = parsed_record;
		}
		
		let bst_glyph_raw: Arc<BstGlyphRaw> = match &glyf_record {
			&GlyfRecord::Parsed(ref glfy_glyph) => {
				if bst_glyphs_raw.contains_key(&glyph_index) {
					bst_glyphs_raw.get(&glyph_index).unwrap().clone()
				} else {
					let min_x = glfy_glyph.bounding_box.x_min as f32 * scaler;
					let min_y = glfy_glyph.bounding_box.y_min as f32 * scaler;
					let max_x = glfy_glyph.bounding_box.x_max as f32 * scaler;
					let max_y = glfy_glyph.bounding_box.y_max as f32 * scaler;
					
					let geometry = match &glfy_glyph.data {
						&glyf::GlyphData::Simple(ref simple) => {
							let mut geometry = Vec::new();
							let mut contour = Vec::new();
							
							for i in 0..simple.coordinates.len() {
								contour.push((
									i,
									simple.coordinates[i].0 as f32 * scaler,
									simple.coordinates[i].1 as f32 * scaler
								));
							
								if simple.end_pts_of_contours.contains(&(i as u16)) {
									for j in 0..contour.len() {
										if !simple.flags[contour[j].0].is_on_curve() {
											let p_i = if j == 0 {
												contour.len() - 1
											} else {
												j - 1
											}; let n_i = if j == contour.len() - 1 {
												0
											} else {
												j + 1
											};
											
											let a = if simple.flags[contour[p_i].0].is_on_curve() {
												(contour[p_i].1, contour[p_i].2)
											} else {
												(
													(contour[p_i].1 + contour[j].1) / 2.0,
													(contour[p_i].2 + contour[j].2) / 2.0
												)
											};
											
											let c = if simple.flags[contour[n_i].0].is_on_curve() {
												(contour[n_i].1, contour[n_i].2)
											} else {
												(
													(contour[n_i].1 + contour[j].1) / 2.0,
													(contour[n_i].2 + contour[j].2) / 2.0
												)
											};
											
											let b = (contour[j].1, contour[j].2);
											
											geometry.push(BstGlyphGeo::Curve([
												BstGlyphPoint { x: a.0, y: a.1 },
												BstGlyphPoint { x: b.0, y: b.1 },
												BstGlyphPoint { x: c.0, y: c.1 }
											]));
										} else {
											let n_i = if j == contour.len() - 1 {
												0
											} else {
												j + 1
											};
											
											if simple.flags[contour[n_i].0].is_on_curve() {
												geometry.push(BstGlyphGeo::Line([
													BstGlyphPoint { x: contour[j].1, y: contour[j].2 },
													BstGlyphPoint { x: contour[n_i].1, y: contour[n_i].2 }
												]));
											}
										}
									}
									
									contour.clear();
								}
							}
					
							geometry
						},
						glyf::GlyphData::Composite { .. } => {
							return Err(BstTextError::src_and_ty(BstTextErrorSrc::Glyph, BstTextErrorTy::UnimplementedDataTy));
						}
					};
					
					let bst_glyph_raw = Arc::new(BstGlyphRaw {
						font: bst_font.clone(),
						index: glyph_index,
						min_x,
						min_y,
						max_x,
						max_y,
						geometry,
						font_height: 16.0,
					});
					
					bst_glyphs_raw.insert(glyph_index, bst_glyph_raw.clone());
					bst_glyph_raw
				}
			},
			&GlyfRecord::Present(_) => panic!("Glyph should already be parsed!"),
			&GlyfRecord::Empty => bst_glyphs_raw.entry(0).or_insert_with(|| Arc::new(BstGlyphRaw::empty(bst_font.clone()))).clone()
		};
		
		bst_glyphs.push(BstGlyph {
			glyph_raw: bst_glyph_raw,
			position: BstGlyphPos {
				x: placement_x,
				y: placement_y,
			},
		});
	}
	
	Ok(bst_glyphs)
}
