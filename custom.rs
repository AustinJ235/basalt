 	#[inline]
    pub fn copy_buffer_with_regions<S, D, T, R>(mut self, source: S, destination: D, regions: R)
                                -> Result<Self, CopyBufferError>
        where S: TypedBufferAccess<Content = T> + Send + Sync + 'static,
              D: TypedBufferAccess<Content = T> + Send + Sync + 'static,
              R: Iterator<Item = (usize, usize, usize)> + Send + Sync + 'static,
              T: ?Sized
    {
    	// TODO: Check regions
        unsafe {
            self.ensure_outside_render_pass()?;
            let infos = check_copy_buffer(self.device(), &source, &destination)?;
            self.inner
                .copy_buffer(source, destination, regions)?;
            Ok(self)
        }
    }

    /// Adds a command that copies from a buffer to an image.
    pub fn copy_buffer_to_image_advance<S, D, Px>(mut self, source: S, destination: D,
                                                     buffer_offset: usize, buffer_w: u32,
                                                     buffer_h: u32, offset: [u32; 3], size: [u32; 3],
                                                     first_layer: u32, num_layers: u32, mipmap: u32)
                                                     -> Result<Self, CopyBufferImageError>
        where S: TypedBufferAccess<Content = [Px]> + Send + Sync + 'static,
              D: ImageAccess + Send + Sync + 'static,
              Format: AcceptsPixels<Px>
    {
        unsafe {
            self.ensure_outside_render_pass()?;
            let byte_size = mem::size_of::<Px>() * Format::rate(&destination.format()) as usize;

            check_copy_buffer_image(self.device(),
                                    &source,
                                    &destination,
                                    CheckCopyBufferImageTy::BufferToImage,
                                    offset,
                                    size,
                                    first_layer,
                                    num_layers,
                                    mipmap)?;

            let copy = UnsafeCommandBufferBuilderBufferImageCopy {
                buffer_offset: buffer_offset,
                buffer_row_length: buffer_w,
                buffer_image_height: buffer_h,
                image_aspect: if destination.has_color() {
                    UnsafeCommandBufferBuilderImageAspect {
                        color: true,
                        depth: false,
                        stencil: false,
                    }
                } else {
                    unimplemented!()
                },
                image_mip_level: mipmap,
                image_base_array_layer: first_layer,
                image_layer_count: num_layers,
                image_offset: [offset[0] as i32, offset[1] as i32, offset[2] as i32],
                image_extent: size,
            };

            self.inner
                .copy_buffer_to_image(source,
                                      destination,
                                      ImageLayout::TransferDstOptimal, // TODO: let choose layout
                                      iter::once(copy))?;
            Ok(self)
        }
    }

    #[inline]
    pub fn draw_vertex_range<V, Gp, S, Pc>(mut self, pipeline: Gp, dynamic: DynamicState, vertices: V, sets: S,
                              constants: Pc, start: u32, end: u32)
                              -> Result<Self, DrawError>
        where Gp: GraphicsPipelineAbstract + VertexSource<V> + Send + Sync + 'static + Clone, // TODO: meh for Clone
              S: DescriptorSetsCollection
    {
        unsafe {
            // TODO: must check that pipeline is compatible with render pass

            self.ensure_inside_render_pass_inline(&pipeline)?;
            check_dynamic_state_validity(&pipeline, &dynamic)?;
            check_push_constants_validity(&pipeline, &constants)?;
            check_descriptor_sets_validity(&pipeline, &sets)?;
            let vb_infos = check_vertex_buffers(&pipeline, vertices)?;

            if let StateCacherOutcome::NeedChange =
                self.state_cacher.bind_graphics_pipeline(&pipeline)
            {
                self.inner.bind_pipeline_graphics(pipeline.clone());
            }

            let dynamic = self.state_cacher.dynamic_state(&dynamic);

            push_constants(&mut self.inner, pipeline.clone(), constants);
            set_state(&mut self.inner, &dynamic);
            descriptor_sets(&mut self.inner,
                            &mut self.state_cacher,
                            true,
                            pipeline.clone(),
                            sets)?;
            vertex_buffers(&mut self.inner,
                           &mut self.state_cacher,
                           vb_infos.vertex_buffers)?;

            debug_assert!(self.graphics_allowed);
            
           	assert!(start <= end);
           	assert!(end < vb_infos.vertex_count);

            self.inner.draw(end - start,
                            vb_infos.instance_count as u32,
                            0,
                            0);
            Ok(self)
        }
    }
