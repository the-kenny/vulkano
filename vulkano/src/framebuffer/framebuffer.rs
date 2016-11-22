// Copyright (c) 2016 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use std::cmp;
use std::error;
use std::fmt;
use std::mem;
use std::ptr;
use std::sync::Arc;

use command_buffer::cmd::CommandsListSink;
use device::Device;
use framebuffer::FramebufferRef;
use framebuffer::RenderPass;
use framebuffer::RenderPassRef;
use framebuffer::RenderPassAttachmentsList;
use framebuffer::RenderPassCompatible;
use image::sys::Layout;
use image::traits::ImageView;
use sync::AccessFlagBits;
use sync::PipelineStages;

use Error;
use OomError;
use VulkanObject;
use VulkanPointers;
use check_errors;
use vk;

/// Contains the list of images attached to a render pass.
///
/// This is a structure that you must pass when you start recording draw commands in a
/// command buffer.
///
/// A framebuffer can be used alongside with any other render pass object as long as it is
/// compatible with the render pass that his framebuffer was created with. You can determine
/// whether two renderpass objects are compatible by calling `is_compatible_with`.
pub struct Framebuffer<Rp = Arc<RenderPass>, A = Box<AttachmentsList>> {
    device: Arc<Device>,
    render_pass: Rp,
    framebuffer: vk::Framebuffer,
    dimensions: [u32; 3],
    resources: A,
}

impl<Rp, A> Framebuffer<Rp, A> {
    /// Builds a new framebuffer.
    ///
    /// The `attachments` parameter depends on which `RenderPassRef` implementation is used.
    pub fn new<Ia>(render_pass: Rp, dimensions: [u32; 3],
                   attachments: Ia) -> Result<Arc<Framebuffer<Rp, A>>, FramebufferCreationError>
        where Rp: RenderPassRef,
              Ia: IntoAttachmentsList<List = A>,
              A: AttachmentsList
    {
        let device = render_pass.device().clone();

        // This function call is supposed to check whether the attachments are valid.
        // For more safety, we do some additional `debug_assert`s below.
        try!(render_pass.desc().check_attachments_list(&attachments));

        let attachments = attachments.into_attachments_list();

        // Checking the dimensions against the limits.
        {
            let limits = render_pass.device().physical_device().limits();
            let limits = [limits.max_framebuffer_width(), limits.max_framebuffer_height(),
                          limits.max_framebuffer_layers()];
            if dimensions[0] > limits[0] || dimensions[1] > limits[1] ||
               dimensions[2] > limits[2]
            {
                return Err(FramebufferCreationError::DimensionsTooLarge);
            }
        }

        let ids = attachments.raw_image_view_handles();

        // FIXME: restore dimensions check
        /*let ids = {
            let mut ids = SmallVec::<[_; 8]>::new();

            for &(ref a, _, _, _) in attachments.iter() {
                debug_assert!(a.identity_swizzle());
                // TODO: add more checks with debug_assert!

                let atch_dims = a.parent().dimensions();
                if atch_dims.width() < dimensions[0] || atch_dims.height() < dimensions[1] ||
                   atch_dims.array_layers() < dimensions[2]      // TODO: wrong, since it must be the array layers of the view and not of the image
                {
                    return Err(FramebufferCreationError::AttachmentTooSmall);
                }

                ids.push(a.inner().internal_object());
            }

            ids
        };*/

        let framebuffer = unsafe {
            let vk = render_pass.device().pointers();

            let infos = vk::FramebufferCreateInfo {
                sType: vk::STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0,   // reserved
                renderPass: render_pass.inner().internal_object(),
                attachmentCount: ids.len() as u32,
                pAttachments: ids.as_ptr(),
                width: dimensions[0],
                height: dimensions[1],
                layers: dimensions[2],
            };

            let mut output = mem::uninitialized();
            try!(check_errors(vk.CreateFramebuffer(device.internal_object(), &infos,
                                                   ptr::null(), &mut output)));
            output
        };

        Ok(Arc::new(Framebuffer {
            device: device,
            render_pass: render_pass,
            framebuffer: framebuffer,
            dimensions: dimensions,
            resources: attachments,
        }))
    }

    /// Returns true if this framebuffer can be used with the specified renderpass.
    #[inline]
    pub fn is_compatible_with<R>(&self, render_pass: &R) -> bool
        where R: RenderPassRef, Rp: RenderPassRef
    {
        self.render_pass.desc().is_compatible_with(render_pass.desc())
    }

    /// Returns the width, height and layers of this framebuffer.
    #[inline]
    pub fn dimensions(&self) -> [u32; 3] {
        self.dimensions
    }

    /// Returns the width of the framebuffer in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.dimensions[0]
    }

    /// Returns the height of the framebuffer in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.dimensions[1]
    }

    /// Returns the number of layers (or depth) of the framebuffer.
    #[inline]
    pub fn layers(&self) -> u32 {
        self.dimensions[2]
    }

    /// Returns the device that was used to create this framebuffer.
    #[inline]
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }

    /// Returns the renderpass that was used to create this framebuffer.
    #[inline]
    pub fn render_pass(&self) -> &Rp {
        &self.render_pass
    }
}

unsafe impl<Rp, A> FramebufferRef for Framebuffer<Rp, A>
    where Rp: RenderPassRef, A: AttachmentsList
{
    #[inline]
    fn inner(&self) -> FramebufferSys {
        FramebufferSys(&self.framebuffer)
    }

    type RenderPassRef = Rp;

    #[inline]
    fn render_pass(&self) -> &Self::RenderPassRef {
        &self.render_pass
    }

    #[inline]
    fn dimensions(&self) -> [u32; 3] {
        self.dimensions
    }

    #[inline]
    fn add_transition<'a>(&'a self, sink: &mut CommandsListSink<'a>) {
        self.resources.add_transition(sink);
    }
}

impl<Rp, A> Drop for Framebuffer<Rp, A> {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let vk = self.device.pointers();
            vk.DestroyFramebuffer(self.device.internal_object(), self.framebuffer, ptr::null());
        }
    }
}

/// Opaque struct that represents a framebuffer without a template parameter.
#[derive(Debug, Copy, Clone)]
pub struct FramebufferSys<'a>(&'a vk::Framebuffer);

unsafe impl<'a> VulkanObject for FramebufferSys<'a> {
    type Object = vk::Framebuffer;

    #[inline]
    fn internal_object(&self) -> vk::Framebuffer {
        *self.0
    }
}

pub unsafe trait AttachmentsList {
    /// Returns the raw handles of the image views of this list.
    // TODO: better return type
    fn raw_image_view_handles(&self) -> Vec<vk::ImageView>;

    /// Returns the minimal dimensions of the views. Returns `None` if the list is empty.
    ///
    /// Must be done for each component individually.
    ///
    /// For example if one view is 256x256x1 and another one is 128x512x2, then this function
    /// should return 128x256x1.
    fn min_dimensions(&self) -> Option<[u32; 3]>;

    fn add_transition<'a>(&'a self, sink: &mut CommandsListSink<'a>);
}

#[derive(Debug, Copy, Clone)]
pub struct EmptyAttachmentsList;
unsafe impl AttachmentsList for EmptyAttachmentsList {
    #[inline]
    fn raw_image_view_handles(&self) -> Vec<vk::ImageView> {
        vec![]
    }

    #[inline]
    fn min_dimensions(&self) -> Option<[u32; 3]> {
        None
    }

    #[inline]
    fn add_transition<'a>(&'a self, sink: &mut CommandsListSink<'a>) {
    }
}

unsafe impl AttachmentsList for () {
    #[inline]
    fn raw_image_view_handles(&self) -> Vec<vk::ImageView> {
        vec![]
    }

    #[inline]
    fn min_dimensions(&self) -> Option<[u32; 3]> {
        None
    }

    #[inline]
    fn add_transition<'a>(&'a self, sink: &mut CommandsListSink<'a>) {
    }
}

pub struct List<A, R> { pub first: A, pub rest: R }

unsafe impl<A, R> AttachmentsList for List<A, R>
    where A: ImageView,
          R: AttachmentsList
{
    #[inline]
    fn raw_image_view_handles(&self) -> Vec<vk::ImageView> {
        let mut list = self.rest.raw_image_view_handles();
        list.insert(0, self.first.inner().internal_object());
        list
    }

    #[inline]
    fn min_dimensions(&self) -> Option<[u32; 3]> {
        let my_view_dims = self.first.parent().dimensions();
        debug_assert_eq!(my_view_dims.depth(), 1);
        let my_view_dims = [my_view_dims.width(), my_view_dims.height(),
                            my_view_dims.array_layers()];       // FIXME: should be the view's layers, not the image's

        match self.rest.min_dimensions() {
            Some(r_dims) => {
                Some([
                    cmp::min(r_dims[0], my_view_dims[0]),
                    cmp::min(r_dims[1], my_view_dims[1]),
                    cmp::min(r_dims[2], my_view_dims[2])
                ])
            },
            None => Some(my_view_dims),
        }
    }

    #[inline]
    fn add_transition<'a>(&'a self, sink: &mut CommandsListSink<'a>) {
        // TODO: "wrong" values
        let stages = PipelineStages {
            color_attachment_output: true,
            late_fragment_tests: true,
            .. PipelineStages::none()
        };
        
        let access = AccessFlagBits {
            color_attachment_read: true,
            color_attachment_write: true,
            depth_stencil_attachment_read: true,
            depth_stencil_attachment_write: true,
            .. AccessFlagBits::none()
        };

        // FIXME: adjust layers & mipmaps with the view's parameters
        sink.add_image_transition(&self.first.parent(), 0, 1, 0, 1, true, Layout::General /* FIXME: wrong */,
                                  stages, access);
        self.rest.add_transition(sink);
    }
}

/// Trait for types that can be turned into a list of attachments.
pub trait IntoAttachmentsList {
    /// The list of attachments.
    type List;

    /// Performs the conversion.
    fn into_attachments_list(self) -> Self::List;
}

/*impl<S, T> IntoAttachmentsList for T where T: AttachmentsList<S> {
    type List = T;

    #[inline]
    fn into_attachments_list(self) -> T {
        self
    }
}*/

impl IntoAttachmentsList for () {
    type List = EmptyAttachmentsList;

    #[inline]
    fn into_attachments_list(self) -> EmptyAttachmentsList {
        EmptyAttachmentsList
    }
}

macro_rules! impl_into_atch_list {
    ($first:ident, $($rest:ident),+) => (
        impl<$first, $($rest),+> IntoAttachmentsList for ($first, $($rest),+) {
            type List = List<$first, <($($rest,)+) as IntoAttachmentsList>::List>;

            #[inline]
            #[allow(non_snake_case)]
            fn into_attachments_list(self) -> Self::List {
                let ($first, $($rest),+) = self;

                List {
                    first: $first,
                    rest: IntoAttachmentsList::into_attachments_list(($($rest,)+))
                }
            }
        }

        impl_into_atch_list!($($rest),+);
    );

    ($alone:ident) => (
        impl<A> IntoAttachmentsList for (A,) {
            type List = List<A, EmptyAttachmentsList>;

            #[inline]
            fn into_attachments_list(self) -> Self::List {
                List { first: self.0, rest: EmptyAttachmentsList }
            }
        }
    );
}

impl_into_atch_list!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z);

/// Error that can happen when creating a framebuffer object.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FramebufferCreationError {
    /// Out of memory.
    OomError(OomError),
    /// The requested dimensions exceed the device's limits.
    DimensionsTooLarge,
    /// One of the attachments has a component swizzle that is different from identity.
    AttachmentNotIdentitySwizzled,
    /// One of the attachments is too small compared to the requested framebuffer dimensions.
    AttachmentTooSmall,
}

impl From<OomError> for FramebufferCreationError {
    #[inline]
    fn from(err: OomError) -> FramebufferCreationError {
        FramebufferCreationError::OomError(err)
    }
}

impl error::Error for FramebufferCreationError {
    #[inline]
    fn description(&self) -> &str {
        match *self {
            FramebufferCreationError::OomError(_) => "no memory available",
            FramebufferCreationError::DimensionsTooLarge => "the dimensions of the framebuffer \
                                                             are too large",
            FramebufferCreationError::AttachmentNotIdentitySwizzled => {
                "one of the attachments has a component swizzle that is different from identity"
            },
            FramebufferCreationError::AttachmentTooSmall => {
                "one of the attachments is too small compared to the requested framebuffer \
                 dimensions"
            },
        }
    }

    #[inline]
    fn cause(&self) -> Option<&error::Error> {
        match *self {
            FramebufferCreationError::OomError(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for FramebufferCreationError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}", error::Error::description(self))
    }
}

impl From<Error> for FramebufferCreationError {
    #[inline]
    fn from(err: Error) -> FramebufferCreationError {
        FramebufferCreationError::from(OomError::from(err))
    }
}

#[cfg(test)]
mod tests {
    use format::R8G8B8A8Unorm;
    use framebuffer::Framebuffer;
    use framebuffer::FramebufferCreationError;
    use image::attachment::AttachmentImage;

    mod example {
        use format::R8G8B8A8Unorm;

        single_pass_renderpass! {
            attachments: {
                color: {
                    load: Clear,
                    store: DontCare,
                    format: R8G8B8A8Unorm,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        }
    }

    #[test]
    fn simple_create() {
        let (device, _) = gfx_dev_and_queue!();

        let render_pass = example::CustomRenderPass::new(&device, &example::Formats {
            color: (R8G8B8A8Unorm, 1)
        }).unwrap();

        let image = AttachmentImage::new(&device, [1024, 768], R8G8B8A8Unorm).unwrap();

        let _ = Framebuffer::new(&render_pass, [1024, 768, 1], example::AList {
            color: image.clone()
        }).unwrap();
    }

    #[test]
    fn framebuffer_too_large() {
        let (device, _) = gfx_dev_and_queue!();

        let render_pass = example::CustomRenderPass::new(&device, &example::Formats {
            color: (R8G8B8A8Unorm, 1)
        }).unwrap();

        let image = AttachmentImage::new(&device, [1024, 768], R8G8B8A8Unorm).unwrap();

        let alist = example::AList { color: image.clone() };
        match Framebuffer::new(&render_pass, [0xffffffff, 0xffffffff, 0xffffffff], alist) {
            Err(FramebufferCreationError::DimensionsTooLarge) => (),
            _ => panic!()
        }
    }

    #[test]
    fn attachment_too_small() {
        let (device, _) = gfx_dev_and_queue!();

        let render_pass = example::CustomRenderPass::new(&device, &example::Formats {
            color: (R8G8B8A8Unorm, 1)
        }).unwrap();

        let image = AttachmentImage::new(&device, [512, 512], R8G8B8A8Unorm).unwrap();

        let alist = example::AList { color: image.clone() };
        match Framebuffer::new(&render_pass, [600, 600, 1], alist) {
            Err(FramebufferCreationError::AttachmentTooSmall) => (),
            _ => panic!()
        }
    }
}
