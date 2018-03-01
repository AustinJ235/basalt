/*
	Buffer Abstract intended for all the 3d Geometry
*/

use super::super::atlas::Atlas;
use std::path::PathBuf;
use std::sync::Arc;
use cgmath;
use super::super::shaders::vs;
use parking_lot::Mutex;
use vulkano::device::{self,Device};
use vulkano::buffer::immutable::ImmutableBuffer;
use vulkano::image::traits::ImageViewAccess;
use misc::BTreeMapExtras;
use std::collections::BTreeMap;
use vulkano::buffer::BufferUsage;
use vulkano::sampler::Sampler;
use std::sync::atomic::{self,AtomicUsize};
use std::thread;
use vulkano::sync::GpuFuture;
use std::sync::Barrier;
use super::{Buffer,Vert};

pub struct BasicBuf {
	atlas: Arc<Atlas>,
	triangles: AtomicUsize,
	up_model: Mutex<(Option<cgmath::Matrix4<f32>>, Option<Arc<Barrier>>)>,
	up_verts: Mutex<(Option<Vec<(usize, Vec<Vert>)>>, Option<Arc<Barrier>>)>,
	up_trans_verts: Mutex<(Option<Vec<(usize, Vec<Vert>)>>, Option<Arc<Barrier>>)>,
	model_buf: Arc<Mutex<Option<Arc<ImmutableBuffer<vs::ty::Other>>>>>,
	verts_bufs: Arc<Mutex<Option<Vec<(Arc<ImageViewAccess + Send + Sync>, Arc<Sampler>, Arc<ImmutableBuffer<[Vert]>>)>>>>,
	trans_verts_bufs: Arc<Mutex<Option<Vec<(Arc<ImageViewAccess + Send + Sync>, Arc<Sampler>, Arc<ImmutableBuffer<[Vert]>>)>>>>
}

impl BasicBuf {
	pub(crate) fn new(atlas: Arc<Atlas>) -> Self {
		BasicBuf {
			atlas: atlas,
			triangles: AtomicUsize::new(0),
			up_model: Mutex::new((None, None)),
			up_verts: Mutex::new((None, None)),
			up_trans_verts: Mutex::new((None, None)),
			model_buf: Arc::new(Mutex::new(None)),
			verts_bufs: Arc::new(Mutex::new(None)),
			trans_verts_bufs: Arc::new(Mutex::new(None)),
		}
	}

	pub fn set_model(&self, model: cgmath::Matrix4<f32>) {
		*self.up_model.lock() = (Some(model), None);
	}
	
	pub fn set_verts(&self, verts: Vec<AbtBasicVert>) {
		let mut out = BTreeMap::new();
		let mut trans_out = BTreeMap::new();
		self.triangles.store(verts.len() / 3, atomic::Ordering::Relaxed);
		
		for vert in verts {
			let (atlas_i, vert, opaque) = vert.into_vert(&self.atlas);
			
			match opaque {
				true => out.get_mut_or_else(&atlas_i, || Vec::new()).push(vert),
				false => trans_out.get_mut_or_else(&atlas_i, || Vec::new()).push(vert),
			}
		}
		
		*self.up_verts.lock() = (Some(out.into_iter().collect()), None);
		*self.up_trans_verts.lock() = (Some(trans_out.into_iter().collect()), None);
	}
	
	/// Using this function will guarantee that both the model and vert bufs are ready before
	/// switching away from the old ones. Useful for when it is crucial that the model and verts
	/// are updated at the same.
	pub fn set_model_and_verts(&self, model: cgmath::Matrix4<f32>, verts: Vec<AbtBasicVert>) {
		let mut out = BTreeMap::new();
		let mut trans_out = BTreeMap::new();
		self.triangles.store(verts.len() / 3, atomic::Ordering::Relaxed);
		
		for vert in verts {
			let (atlas_i, vert, opaque) = vert.into_vert(&self.atlas);
			
			match opaque {
				true => out.get_mut_or_else(&atlas_i, || Vec::new()).push(vert),
				false => trans_out.get_mut_or_else(&atlas_i, || Vec::new()).push(vert),
			}
		}
		
		let barrier = Some(Arc::new(Barrier::new(3)));
		
		{
			let mut up_model = self.up_model.lock();
			let mut up_verts = self.up_verts.lock();
			let mut up_trans_verts = self.up_trans_verts.lock();
		
			if let Some(barrier) = up_model.1.take() {
				barrier.wait();
			} if let Some(barrier) = up_verts.1.take() {
				barrier.wait();
			} if let Some(barrier) = up_trans_verts.1.take() {
				barrier.wait();
			}
		
			*up_model = (Some(model), barrier.clone());
			*up_verts = (Some(out.into_iter().collect()), barrier.clone());
			*up_trans_verts = (Some(trans_out.into_iter().collect()), barrier.clone());
		}
	}
}

impl Buffer for BasicBuf {
	fn triangles(&self) -> usize {
		self.triangles.load(atomic::Ordering::Relaxed)
	}

	fn draw(&self, device: Arc<Device>, queue: Arc<device::Queue>) ->
		Option<(
			Arc<ImmutableBuffer<vs::ty::Other>>,
			Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>, Vec<(
				Arc<ImageViewAccess + Send + Sync>,
				Arc<Sampler>,
				Arc<ImmutableBuffer<[Vert]>>
			)>,
		)>
	{
		{ // Update Model
			let (up_model_, barrier_) = {
				let mut up_model = self.up_model.lock();
				(up_model.0.take(), up_model.1.take())
			}; match up_model_ {
				Some(up_model) => {
					let _queue = queue.clone();
					let _model_buf_ = self.model_buf.clone();
					
					thread::spawn(move || {
						let (buf, future) = ImmutableBuffer::from_data(
							vs::ty::Other {
								model: up_model.into(),
							}, BufferUsage::uniform_buffer(),
							_queue
						).unwrap();
						
						future.flush().unwrap();
						future.then_signal_fence().wait(None).unwrap();
						
						if let Some(barrier) = barrier_ {
							barrier.wait();
						}
						
						*_model_buf_.lock() = Some(buf);
					});
				}, None => if let Some(barrier) = barrier_ {
					barrier.wait();
				}
			}
		}
		
		{ // Update Opaque Verts
			let (up_verts_, barrier_) = {
				let mut up_verts = self.up_verts.lock();
				(up_verts.0.take(), up_verts.1.take())
			}; match up_verts_ {
				Some(up_verts) => {
					let _queue = queue.clone();
					let _verts_bufs_ = self.verts_bufs.clone();
					let _device = device.clone();
					let _atlas = self.atlas.clone();
					
					thread::spawn(move || {
						let sampler_for_null = Sampler::simple_repeat_linear_no_mipmap(_device); 
						let done_verts_ = Arc::new(Mutex::new(Some(Vec::new())));
						let mut threads = Vec::new();
						
						for (atlas_i, verts) in up_verts {
							let (image, sampler) = match atlas_i {
								0 => (_atlas.null_img(_queue.clone()), sampler_for_null.clone()),
								_ => match _atlas.image_and_sampler(atlas_i) {
									Some(some) => some,
									None => (_atlas.null_img(_queue.clone()), sampler_for_null.clone())
								}
							};
							
							let _done_verts_ = done_verts_.clone();
							let __queue = _queue.clone();

							threads.push(thread::spawn(move || {
								let (buf, future) = ImmutableBuffer::from_iter(verts.into_iter(), BufferUsage::vertex_buffer(), __queue).unwrap();
								future.flush().unwrap();
								future.then_signal_fence().wait(None).unwrap();
								_done_verts_.lock().as_mut().unwrap().push((image, sampler, buf));
							}));
						}
						
						for thread in threads {
							thread.join().unwrap();
						}
						
						if let Some(barrier) = barrier_ {
							barrier.wait();
						}
						
						*_verts_bufs_.lock() = done_verts_.lock().take();
						
					});
				
				}, None => if let Some(barrier) = barrier_ {
					barrier.wait();
				}
			}
		}
		
		{ // Update Transparent Verts
			let (up_verts_, barrier_) = {
				let mut up_verts = self.up_trans_verts.lock();
				(up_verts.0.take(), up_verts.1.take())
			}; match up_verts_ {
				Some(up_verts) => {
					let _queue = queue.clone();
					let _verts_bufs_ = self.trans_verts_bufs.clone();
					let _device = device.clone();
					let _atlas = self.atlas.clone();
					
					thread::spawn(move || {
						let sampler_for_null = Sampler::simple_repeat_linear_no_mipmap(_device); 
						let done_verts_ = Arc::new(Mutex::new(Some(Vec::new())));
						let mut threads = Vec::new();
						
						for (atlas_i, verts) in up_verts {
							let (image, sampler) = match atlas_i {
								0 => (_atlas.null_img(_queue.clone()), sampler_for_null.clone()),
								_ => match _atlas.image_and_sampler(atlas_i) {
									Some(some) => some,
									None => (_atlas.null_img(_queue.clone()), sampler_for_null.clone())
								}
							};
							
							let _done_verts_ = done_verts_.clone();
							let __queue = _queue.clone();

							threads.push(thread::spawn(move || {
								let (buf, future) = ImmutableBuffer::from_iter(verts.into_iter(), BufferUsage::vertex_buffer(), __queue).unwrap();
								future.flush().unwrap();
								future.then_signal_fence().wait(None).unwrap();
								_done_verts_.lock().as_mut().unwrap().push((image, sampler, buf));
							}));
						}
						
						for thread in threads {
							thread.join().unwrap();
						}
						
						if let Some(barrier) = barrier_ {
							barrier.wait();
						}
						
						*_verts_bufs_.lock() = done_verts_.lock().take();
						
					});
				
				}, None => if let Some(barrier) = barrier_ {
					barrier.wait();
				}
			}
		}
		
		Some((
			match self.model_buf.lock().clone() {
				Some(some) => some,
				None => return None
			}, match self.verts_bufs.lock().clone() {
				Some(some) => some,
				None => return None
			}, match self.trans_verts_bufs.lock().clone() {
				Some(some) => some,
				None => return None
			}
		))
	}
}		

#[derive(Clone,PartialEq)]
pub struct AbtBasicVert {
	pub position: (f32, f32, f32),
	pub normal: (f32, f32, f32),
	pub color: Option<(f32, f32, f32, f32)>,
	pub coords: Option<(f32, f32)>,
	pub texture: Option<PathBuf>,
}

impl AbtBasicVert {
	pub fn pos_norm_color(position: (f32, f32, f32), normal: (f32, f32, f32), color: (f32, f32, f32, f32)) -> Self {
		AbtBasicVert {
			position: position,
			normal: normal,
			color: Some(color),
			coords: None,
			texture: None,
		}
	}
	
	pub fn pos_norm_tex<P: Into<PathBuf>>(position: (f32, f32, f32), normal: (f32, f32, f32), coords: (f32, f32), tex: P) -> Self {
		AbtBasicVert {
			position: position,
			normal: normal,
			color: None,
			coords: Some(coords),
			texture: Some(tex.into())
		}
	}
	
	fn into_vert(self, atlas: &Arc<Atlas>) -> (usize, Vert, bool) {
		if self.color.is_some() {
			let opaque = {
				if self.color.as_ref().unwrap().3 < 1.0 {
					false
				} else {
					true
				}
			};
		
			(0, Vert {
				position: self.position,
				normal: self.normal,
				color: self.color.unwrap(),
				tex_info: (0.0, 0.0, 0.0, 0.0),
				ty: 0,
			}, opaque)
		} else {
			let coords_info = atlas.coords_with_path(self.texture.unwrap()).unwrap();
			let coords = self.coords.unwrap();
		
			(coords_info.atlas_i, Vert {
				position: self.position,
				normal: self.normal,
				color: (coords.0, coords.1, 0.0, 0.0),
				tex_info: (coords_info.x as f32, coords_info.y as f32, coords_info.w as f32, coords_info.h as f32),
				ty: 1
			}, coords_info.opaque)
		}
	}
}

