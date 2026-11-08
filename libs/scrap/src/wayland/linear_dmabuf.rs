//! CPU-only linear DMA-BUF import. SPA/GstMemory sizes may describe an opaque
//! handle, so a fresh, kernel-sized-checked mapping owns its own duplicated fd.
use gstreamer as gst;
use gst::glib::translate::{from_glib_full, ToGlibPtr};
use hbb_common::libc;
use std::convert::TryFrom;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

#[link(name = "gstallocators-1.0")]
extern "C" {
    fn gst_dmabuf_memory_get_fd(memory: *mut gst::gst_sys::GstMemory) -> libc::c_int;
    fn gst_fd_allocator_new() -> *mut gst::gst_sys::GstAllocator;
    fn gst_fd_allocator_alloc(
        allocator: *mut gst::gst_sys::GstAllocator,
        fd: libc::c_int,
        size: usize,
        flags: libc::c_int,
    ) -> *mut gst::gst_sys::GstMemory;
}

#[repr(C)]
struct DmaBufSync {
    flags: u64,
}
// Linux x86-64 _IOW('b', 0, struct dma_buf_sync); other ABIs fail closed below.
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
type SyncCall = fn(libc::c_int, bool) -> Result<(), &'static str>;

fn sync_read(fd: libc::c_int, end: bool) -> Result<(), &'static str> {
    let mut request = DmaBufSync { flags: 1 | if end { 4 } else { 0 } };
    for _ in 0..3 {
        // SAFETY: fd is kept alive by the borrowed original buffer, request is
        // the kernel's repr(C) u64 payload, and this route is x86-64 Linux only.
        if unsafe { libc::ioctl(fd, DMA_BUF_IOCTL_SYNC, &mut request) } == 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) | Some(libc::EAGAIN) => continue,
            _ => break,
        }
    }
    Err(if end { "dmabuf-sync-end-failed" } else { "dmabuf-sync-start-failed" })
}

struct ReadAccess {
    fd: libc::c_int,
    sync: SyncCall,
    active: bool,
}

impl ReadAccess {
    fn begin(fd: libc::c_int, sync: SyncCall) -> Result<Self, &'static str> {
        sync(fd, false)?;
        Ok(Self { fd, sync, active: true })
    }

    fn finish(mut self) -> Result<(), &'static str> {
        self.active = false;
        (self.sync)(self.fd, true)
    }
}

impl Drop for ReadAccess {
    fn drop(&mut self) {
        if self.active && (self.sync)(self.fd, true).is_err() {
            // Only the unwind path cannot return an END error to the caller.
            hbb_common::log::error!("linear-cpu-map-v2 error=dmabuf-sync-end-failed");
        }
    }
}

fn with_read_access<T>(
    fd: libc::c_int,
    sync: SyncCall,
    action: impl FnOnce() -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let access = ReadAccess::begin(fd, sync)?;
    let result = action();
    access.finish()?;
    result
}

pub(super) fn copy_visible(
    buffer: &gst::BufferRef,
    width: usize,
    height: usize,
    stride: usize,
    meta_offset: usize,
    crop: Option<(u32, u32, u32, u32)>,
    output: &mut Vec<u8>,
) -> Result<(usize, usize), &'static str> {
    copy_visible_with_sync(buffer, width, height, stride, meta_offset, crop, output, sync_read)
}

fn copy_visible_with_sync(
    buffer: &gst::BufferRef,
    width: usize,
    height: usize,
    stride: usize,
    meta_offset: usize,
    crop: Option<(u32, u32, u32, u32)>,
    output: &mut Vec<u8>,
    sync: SyncCall,
) -> Result<(usize, usize), &'static str> {
    // Keep reusable capacity, but never leave a frame in output after an error
    // or unwind, including an END failure after pixels have already been copied.
    let mut packed = std::mem::take(output);
    packed.clear();
    let result = (|| {
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err("dmabuf-unsupported-sync-abi");
        }
        if width == 0 || height == 0 || buffer.n_memory() != 1 {
            return Err("dmabuf-layout-invalid");
        }
        let memory = buffer.peek_memory(0);
        if !memory.is_type("dmabuf") {
            return Err("dmabuf-memory-type-invalid");
        }
        let row = width.checked_mul(4).ok_or("dmabuf-span-overflow")?;
        if stride < row {
            return Err("dmabuf-layout-invalid");
        }
        // The new wrapper maps fd offset zero. Apply the old memory origin and
        // the buffer-relative VideoMeta offset once, not to an already-shifted map.
        let origin = memory.get_offset().checked_add(meta_offset)
            .ok_or("dmabuf-span-overflow")?;
        let required = (height - 1).checked_mul(stride)
            .and_then(|n| n.checked_add(origin))
            .and_then(|n| n.checked_add(row))
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or("dmabuf-span-overflow")?;
        // SAFETY: the checked GstMemory is borrowed from buffer for this whole
        // function. The native getter returns a borrowed fd, never ownership.
        let fd = unsafe { gst_dmabuf_memory_get_fd(memory.as_mut_ptr()) };
        if fd < 0 {
            return Err("dmabuf-fd-invalid");
        }
        // Linux v7.2.14 dma_buf_llseek(END,0) returns capacity without changing f_pos.
        // CUR is unsupported; neither a guessed size nor a reset is appropriate.
        let capacity = unsafe { libc::lseek(fd, 0, libc::SEEK_END) };
        let capacity = usize::try_from(capacity).ok().filter(|&n| n != 0)
            .ok_or("dmabuf-size-query-failed")?;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page = usize::try_from(page).ok().filter(|&n| n != 0)
            .ok_or("dmabuf-page-size-invalid")?;
        if required > capacity / page * page {
            return Err("dmabuf-span-exceeds-fd");
        }
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err("dmabuf-fd-dup-failed");
        }
        // SAFETY: fcntl returned a fresh owned fd. Every failure before native
        // ownership transfer closes it through OwnedFd, never the original fd.
        let owned = unsafe { OwnedFd::from_raw_fd(duplicate) };
        let allocator_ptr = unsafe { gst_fd_allocator_new() };
        if allocator_ptr.is_null() {
            return Err("dmabuf-allocator-unavailable");
        }
        let allocator: gst::Allocator = unsafe { from_glib_full(allocator_ptr) };
        let wrapper_ptr = unsafe {
            gst_fd_allocator_alloc(allocator.to_glib_none().0, owned.as_raw_fd(), required, 0)
        };
        if wrapper_ptr.is_null() {
            return Err("dmabuf-wrapper-allocation-failed");
        }
        let _transferred_fd = owned.into_raw_fd();
        // SAFETY: nonnull transfer-full GstMemory from the allocator. It alone
        // closes the duplicate; map/END finish while original buffer is still held.
        let wrapper = unsafe { gst::Memory::from_glib_full(wrapper_ptr) };
        with_read_access(fd, sync, || {
            let map = wrapper.map_readable().map_err(|_| "dmabuf-map-failed")?;
            super::linear_cpu::copy_visible(
                map.as_slice(), width, height, stride, origin, crop, &mut packed,
            ).map_err(|tag| match tag {
                "XR24 metadata exceeds mapped memory" => "dmabuf-copy-span-short",
                "XR24 crop is outside the frame" => "dmabuf-crop-invalid",
                "XR24 output allocation failed" => "dmabuf-output-allocation-failed",
                tag if tag.ends_with("overflow") => "dmabuf-span-overflow",
                _ => "dmabuf-layout-invalid",
            })
            // map drops here before the outer access guard performs checked END.
        })
        // wrapper closes the duplicate only after END has finished or unwound.
    })();
    if result.is_err() {
        packed.clear();
    }
    *output = packed;
    result
}

#[cfg(test)]
pub(super) fn sync_for_test(fd: libc::c_int, end: bool) -> Result<(), &'static str> {
    sync_read(fd, end)
}

#[cfg(test)]
pub(super) fn copy_for_test(
    buffer: &gst::BufferRef, width: usize, height: usize, stride: usize,
    meta_offset: usize, output: &mut Vec<u8>, sync: SyncCall,
) -> Result<(usize, usize), &'static str> {
    copy_visible_with_sync(buffer, width, height, stride, meta_offset, None, output, sync)
}

#[cfg(test)]
#[path = "linear_dmabuf_tests.rs"]
mod tests;
