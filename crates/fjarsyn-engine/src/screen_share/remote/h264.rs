/// Reports whether an H.264 byte stream contains the requested NAL-unit type.
pub(super) fn contains_nal_type(data: &[u8], target: u8) -> bool {
    let mut offset = 0;
    let mut found_annex_b = false;
    while offset + 3 <= data.len() {
        let start_code_len = if data[offset..].starts_with(&[0, 0, 0, 1]) {
            Some(4)
        } else if data[offset..].starts_with(&[0, 0, 1]) {
            Some(3)
        } else {
            None
        };
        if let Some(start_code_len) = start_code_len {
            found_annex_b = true;
            let nal_offset = offset + start_code_len;
            if data.get(nal_offset).is_some_and(|header| header & 0x1f == target) {
                return true;
            }
            offset = nal_offset.saturating_add(1);
        } else {
            offset += 1;
        }
    }
    if found_annex_b {
        return false;
    }

    // Accept AVCC length-prefixed access units as well as a single raw NAL.
    let mut offset = 0;
    let mut parsed_avcc = false;
    while offset + 4 <= data.len() {
        let size =
            u32::from_be_bytes(data[offset..offset + 4].try_into().expect("four bytes")) as usize;
        let nal_offset = offset + 4;
        let Some(end) = nal_offset.checked_add(size).filter(|end| *end <= data.len()) else {
            break;
        };
        if size == 0 {
            break;
        }
        parsed_avcc = true;
        if data[nal_offset] & 0x1f == target {
            return true;
        }
        offset = end;
    }
    if parsed_avcc && offset == data.len() {
        return false;
    }

    data.first().is_some_and(|header| header & 0x1f == target)
}
