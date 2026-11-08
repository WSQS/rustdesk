/// Pack visible little-endian XR24 rows; reject ambiguous layouts instead of reading padding.
pub(super) fn copy_visible(
    data: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    offset: usize,
    crop: Option<(u32, u32, u32, u32)>,
    output: &mut Vec<u8>,
) -> Result<(usize, usize), &'static str> {
    if !cfg!(target_endian = "little") || width == 0 || height == 0 {
        return Err("Linear XR24 requires a nonempty little-endian frame");
    }
    let row_bytes = width.checked_mul(4).ok_or("XR24 row size overflow")?;
    if stride < row_bytes {
        return Err("XR24 stride is shorter than a visible row");
    }
    let required = (height - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(offset))
        .and_then(|n| n.checked_add(row_bytes))
        .ok_or("XR24 frame span overflow")?;
    if required > data.len() {
        return Err("XR24 metadata exceeds mapped memory");
    }
    let (x, y, w, h) = match crop {
        Some((x, y, w, h)) => (x as usize, y as usize, w as usize, h as usize),
        None => (0, 0, width, height),
    };
    if w == 0 || h == 0 || x > width || y > height
        || w > width - x || h > height - y
    {
        return Err("XR24 crop is outside the frame");
    }
    let packed_row = w.checked_mul(4).ok_or("XR24 cropped row overflow")?;
    let packed_size = packed_row.checked_mul(h).ok_or("XR24 output size overflow")?;
    output.clear();
    output.try_reserve(packed_size).map_err(|_| "XR24 output allocation failed")?;
    for row in 0..h {
        let start = offset + (y + row) * stride + x * 4;
        output.extend_from_slice(&data[start..start + packed_row]);
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::copy_visible;

    #[test]
    fn packs_visible_rows_without_prefix_row_or_tail_padding() {
        let mut data = vec![0xaa; 32];
        data[3..11].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        data[15..23].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let mut output = Vec::new();
        assert_eq!(copy_visible(&data, 2, 2, 12, 3, None, &mut output), Ok((2, 2)));
        assert_eq!(output, (1..=16).collect::<Vec<_>>());
    }

    #[test]
    fn crops_using_original_stride() {
        let data = (0u8..40).collect::<Vec<_>>();
        let mut output = Vec::new();
        assert_eq!(copy_visible(&data, 3, 2, 16, 2, Some((1, 1, 2, 1)), &mut output), Ok((2, 1)));
        assert_eq!(output, data[22..30]);
    }

    #[test]
    fn rejects_short_overflowing_or_out_of_bounds_layouts() {
        let mut output = Vec::new();
        for (w, h, stride, offset, crop) in [
            (0, 1, 4, 0, None),
            (1, 0, 4, 0, None),
            (2, 2, 7, 0, None),
            (2, 2, 16, 0, None),
            (usize::MAX, 1, usize::MAX, 0, None),
            (1, usize::MAX, usize::MAX, 0, None),
            (1, 1, 4, usize::MAX, None),
            (1, 1, 4, 0, Some((1, 0, 1, 1))),
        ] {
            assert!(copy_visible(&[0; 16], w, h, stride, offset, crop, &mut output).is_err());
        }
    }
}
