use cgmath;
use std::f32::consts::PI;
use parking_lot::{Mutex,RwLock};
use crossbeam::sync::MsQueue;
use super::sync::q_share::QShare;
use std::sync::Arc;
use cgmath::InnerSpace;
use cgmath::SquareMatrix;

pub struct Camera {
	data: Mutex<Data>,
	queue: MsQueue<Command>,
	pos_qshare: RwLock<QShare<(f32, f32, f32)>>,
	scale: f32,
	sun_angle: RwLock<f32>,
}

#[derive(Default,Clone)]
pub struct Data {
	pub rot_x: f32,
	pub rot_y: f32,
	pub rot_z: f32,
	pub pos_x: f32,
	pub pos_y: f32,
	pub pos_z: f32,
}

pub enum Command {
	SetPos(f32, f32, f32),
	SetRot(f32, f32, f32),
	MoveForward(f32),
	MoveBackward(f32),
	MoveLeft(f32),
	MoveRight(f32),
	MoveUp(f32),
	MoveDown(f32),
	LookUp(f32),
	LookDown(f32),
	LookLeft(f32),
	LookRight(f32),
	TiltLeft(f32),
	TiltRight(f32),
}

impl Camera {
	pub(crate) fn new() -> Self {
		Camera {
			data: Mutex::new(Data::default()),
			queue: MsQueue::new(),
			pos_qshare: RwLock::new(QShare::new()),
			scale: 0.25,
			sun_angle: RwLock::new(160.0),
		}
	}
	
	pub fn set_sun_angle(&self, angle: f32) {
		*self.sun_angle.write() = angle;
	}
	
	pub fn sun_direction(&self) -> cgmath::Vector4<f32> {
		let sun_angle = self.sun_angle.read().clone();
		let off_y = f32::sin(sun_angle.to_radians());
		let off_z = f32::cos(sun_angle.to_radians());
		let dir = cgmath::Vector4::new(0.0, off_y, -off_z, 0.0);
		dir.normalize()
	}
	
	pub fn data(&self) -> Data {
		self.data.lock().clone()
	}
	
	pub fn pos_queue(&self) -> Arc<MsQueue<(f32, f32, f32)>> {
		self.pos_qshare.write().new_queue()
	}
	
	pub(crate) fn process_queue(&self) {
		let mut data = self.data.lock();
		
		loop {
		
			let cmd = match self.queue.try_pop() {
				Some(some) => some,
				None => break
			}; match cmd {
				Command::SetPos(x, y, z) => {
					data.pos_x = x;
					data.pos_y = y;
					data.pos_z = z;
				}, Command::SetRot(x, y, z) => {
					data.rot_x = x;
					data.rot_y = y;
					data.rot_z = z;
				}, Command::MoveForward(amt) => {
					data.pos_y += f32::sin(data.rot_x * PI / 180.0) * amt;
					data.pos_z += f32::cos(data.rot_y * PI / 180.0) * amt;
					data.pos_x += f32::sin(data.rot_y * PI / 180.0) * amt;
				}, Command::MoveBackward(amt) => {
					data.pos_y -= f32::sin(data.rot_x * PI / 180.0) * amt;
					data.pos_z -= f32::cos(data.rot_y * PI / 180.0) * amt;
					data.pos_x -= f32::sin(data.rot_y * PI / 180.0) * amt;
				}, Command::MoveLeft(amt) => {
					data.pos_z += f32::cos((data.rot_y - 90.0) * PI / 180.0) * amt;
					data.pos_x += f32::sin((data.rot_y - 90.0) * PI / 180.0) * amt;
				}, Command::MoveRight(amt) => {
					data.pos_z += f32::cos((data.rot_y + 90.0) * PI / 180.0) * amt;
					data.pos_x += f32::sin((data.rot_y + 90.0) * PI / 180.0) * amt;
				}, Command::MoveUp(amt) => {
					data.pos_y += amt;
				}, Command::MoveDown(amt) => {
					data.pos_y -= amt;
				}, Command::LookUp(amt) => {
					if data.rot_x-amt < -90.0 {
						data.rot_x = -90.0;
					} else if data.rot_x-amt > 90.0 {
						data.rot_x = 90.0;
					} else {
						data.rot_x -= amt;
					}
				}, Command::LookDown(mut amt) => {
					amt *= -1.0;
					
					if data.rot_x-amt < -90.0 {
						data.rot_x = -90.0;
					} else if data.rot_x-amt > 90.0 {
						data.rot_x = 90.0;
					} else {
						data.rot_x -= amt;
					}
				}, Command::LookLeft(amt) => {
					if data.rot_y-amt <= -360.0 {
						data.rot_y -= amt;
						data.rot_y += 360.0;
					} else if data.rot_y-amt >= 360.0 {
						data.rot_y -= amt;
						data.rot_y -= 360.0;
					} else {
						data.rot_y -= amt;
					}
				}, Command::LookRight(mut amt) => {
					amt *= -1.0;
					
					if data.rot_y-amt <= -360.0 {
						data.rot_y -= amt;
						data.rot_y += 360.0;
					} else if data.rot_y-amt >= 360.0 {
						data.rot_y -= amt;
						data.rot_y -= 360.0;
					} else {
						data.rot_y -= amt;
					}
				}, Command::TiltLeft(amt) => {
					if data.rot_z-amt <= -360.0 {
						data.rot_z -= amt;
						data.rot_z += 360.0;
					} else if data.rot_z-amt >= 360.0 {
						data.rot_z -= amt;
						data.rot_z -= 360.0;
					} else {
						data.rot_z -= amt;
					}
				}, Command::TiltRight(mut amt) => {
					amt *= -1.0;
					
					if data.rot_z-amt <= -360.0 {
						data.rot_z -= amt;
						data.rot_z += 360.0;
					} else if data.rot_z-amt >= 360.0 {
						data.rot_z -= amt;
						data.rot_z -= 360.0;
					} else {
						data.rot_z -= amt;
					}
				}
			}
		}
		
		self.pos_qshare.read().push_all((data.pos_x, data.pos_y, data.pos_z));
	}
	
	pub fn screen_to_world(&self, x: u32, y: u32, depth: f32, size_x: u32, size_y: u32) -> (f32, f32, f32) {
		let mut point = cgmath::Vector4::new(
			((x as f32 / size_x as f32) * 2.0) - 1.0,
			((y as f32 / size_y as f32) * 2.0) - 1.0,
			depth,
			1.0
		);
		
		point = self.projection_matrix(size_x, size_y).invert().unwrap() * point;
		point /= point.w;
		point = self.view_matrix().invert().unwrap() * point;
		point /= point.w;
		
		(point.x, point.y, point.z)
	}		 
	
	pub fn projection_matrix(&self, size_x: u32, size_y: u32) -> cgmath::Matrix4<f32> {
		cgmath::perspective(cgmath::Deg(90.0), size_x as f32 / size_y as f32, 0.1, 1000.0)
	}
	
	pub fn shadow_vp(&self, _: u32, _: u32) -> cgmath::Matrix4<f32> {
		const SUN_DIST: f32 = 100.0;
		let sun_angle = self.sun_angle.read().clone();
		
		let rot_x = cgmath::Matrix4::from_angle_x(cgmath::Deg(-sun_angle));
		let rot_y = cgmath::Matrix4::from_angle_y(cgmath::Deg(180.0));
		let rot_z = cgmath::Matrix4::from_angle_z(cgmath::Deg(180.0));
		let ortho = cgmath::ortho(-200.0, 200.0, -200.0, 200.0, -400.0, 200.0);
		let scale = cgmath::Matrix4::from_scale(self.scale);
		let off_y = f32::sin(sun_angle.to_radians()) * SUN_DIST;
		let off_x = f32::cos(sun_angle.to_radians()) * SUN_DIST;
		let data = self.data.lock().clone();
		
		let trans = cgmath::Matrix4::from_translation(cgmath::Vector3::new(
			-data.pos_x * self.scale,
			//(-data.pos_x - off_x) * self.scale,
			-off_y,
			(-data.pos_z - off_x) * self.scale,
			//-data.pos_z * self.scale
		));
		
		ortho * rot_x * rot_y * rot_z * trans * scale
	}
	
	pub fn direction(&self) -> cgmath::Vector3<f32> {
		let data = self.data.lock().clone();
		let vector = cgmath::Vector3::new(
			f32::sin(data.rot_x * PI / 180.0),
			f32::cos(data.rot_y * PI / 180.0),
			f32::sin(data.rot_y * PI / 180.0)
		); vector.normalize()
	}

	pub fn view_matrix(&self) -> cgmath::Matrix4<f32> {
		let data = self.data.lock().clone();
		let rot_x = cgmath::Matrix4::from_angle_x(cgmath::Deg(data.rot_x));
		let rot_y = cgmath::Matrix4::from_angle_y(cgmath::Deg(180.0+data.rot_y));
		let rot_z = cgmath::Matrix4::from_angle_z(cgmath::Deg(180.0+data.rot_z));
		let trans = cgmath::Matrix4::from_translation(cgmath::Vector3::new(-data.pos_x*self.scale, -data.pos_y*self.scale, -data.pos_z*self.scale));
		let scale = cgmath::Matrix4::from_scale(self.scale);
		rot_x * rot_y * rot_z * trans * scale
	}
	
	pub fn rotation_matrix(&self) -> cgmath::Matrix4<f32> {
		let data = self.data.lock().clone();
		let rot_x = cgmath::Matrix4::from_angle_x(cgmath::Deg(data.rot_x));
		let rot_y = cgmath::Matrix4::from_angle_y(cgmath::Deg(180.0+data.rot_y));
		let rot_z = cgmath::Matrix4::from_angle_z(cgmath::Deg(180.0+data.rot_z));
		let scale = cgmath::Matrix4::from_scale(self.scale);
		rot_x * rot_y * rot_z * scale
	}
	
	pub fn command(&self, cmd: Command) {
		self.queue.push(cmd);
	}
	
	pub fn get_pos(&self) -> (f32, f32, f32) {
		let data = self.data.lock();
		(data.pos_x, data.pos_y, data.pos_z)
	}
	
	pub fn get_rot(&self) -> (f32, f32, f32) {
		let data = self.data.lock();
		(data.rot_x, data.rot_y, data.rot_z)
	}
}

