use std::fmt;

const SPS_NAL_TYPE: u8 = 7;
const PPS_NAL_TYPE: u8 = 8;
const IDR_NAL_TYPE: u8 = 5;

/// A malformed or non-Annex-B H.264 access unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InvalidAccessUnit;

impl fmt::Display for InvalidAccessUnit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("malformed Annex-B H.264 access unit")
    }
}

/// Fail-closed decoder-input gate for one H.264 continuity interval.
#[derive(Debug, Default)]
pub(super) struct DecoderBootstrapGate {
    synchronized: bool,
}

impl DecoderBootstrapGate {
    pub(super) const fn is_synchronized(&self) -> bool {
        self.synchronized
    }

    pub(super) fn reset(&mut self) {
        self.synchronized = false;
    }

    /// Validates `data` and reports whether it may enter the decoder. Once
    /// opened, the gate still validates framing so malformed media re-arms the
    /// caller's fail-closed recovery path.
    pub(super) fn accepts(&mut self, data: &[u8]) -> Result<bool, InvalidAccessUnit> {
        let bootstrap = is_decoder_bootstrap(data)?;
        if !self.synchronized && bootstrap {
            self.synchronized = true;
        }
        Ok(self.synchronized)
    }
}

/// Validates one depacketized Annex-B access unit and reports whether it is a
/// self-contained decoder bootstrap boundary.
///
/// The WebRTC H.264 depacketizer always produces Annex-B. Rejecting raw and
/// AVCC-shaped input here keeps malformed media from accidentally opening the
/// decoder gate. A bootstrap must carry SPS, PPS, and an IDR slice together.
pub(super) fn is_decoder_bootstrap(data: &[u8]) -> Result<bool, InvalidAccessUnit> {
    let Some((mut start, mut start_code_len)) = find_start_code(data, 0) else {
        return Err(InvalidAccessUnit);
    };
    if data[..start].iter().any(|byte| *byte != 0) {
        return Err(InvalidAccessUnit);
    }

    let mut found_sps = false;
    let mut found_pps_after_sps = false;
    let mut found_bootstrap_idr = false;
    let mut found_vcl_before_bootstrap = false;

    loop {
        let nal_start = start.checked_add(start_code_len).ok_or(InvalidAccessUnit)?;
        let next = find_start_code(data, nal_start);
        let mut nal_end = next.map_or(data.len(), |(offset, _)| offset);
        while nal_end > nal_start && data[nal_end - 1] == 0 {
            nal_end -= 1;
        }
        let nal = data.get(nal_start..nal_end).ok_or(InvalidAccessUnit)?;
        if nal.len() < 2 || nal[0] & 0x80 != 0 {
            return Err(InvalidAccessUnit);
        }

        let nal_type = nal[0] & 0x1f;
        if !(1..=23).contains(&nal_type) {
            return Err(InvalidAccessUnit);
        }
        match nal_type {
            SPS_NAL_TYPE => found_sps = true,
            PPS_NAL_TYPE if found_sps => found_pps_after_sps = true,
            IDR_NAL_TYPE if found_sps && found_pps_after_sps => {
                found_bootstrap_idr = true;
            }
            1..=5 if !found_bootstrap_idr => found_vcl_before_bootstrap = true,
            _ => {}
        }

        let Some((next_start, next_start_code_len)) = next else {
            break;
        };
        start = next_start;
        start_code_len = next_start_code_len;
    }

    Ok(found_bootstrap_idr && !found_vcl_before_bootstrap)
}

fn find_start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut offset = from;
    while offset < data.len() {
        if data[offset..].starts_with(&[0, 0, 0, 1]) {
            return Some((offset, 4));
        }
        if data[offset..].starts_with(&[0, 0, 1]) {
            return Some((offset, 3));
        }
        offset += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{DecoderBootstrapGate, is_decoder_bootstrap};

    const SPS: &[u8] = &[0, 0, 0, 1, 0x67, 0x42];
    const PPS: &[u8] = &[0, 0, 1, 0x68, 0xce];
    const IDR: &[u8] = &[0, 0, 0, 1, 0x65, 0x88];
    const P_SLICE: &[u8] = &[0, 0, 1, 0x61, 0x99];

    fn access_unit(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().flat_map(|part| part.iter().copied()).collect()
    }

    #[test]
    fn requires_sps_pps_and_idr_in_one_access_unit() {
        assert!(is_decoder_bootstrap(&access_unit(&[SPS, PPS, IDR])).unwrap());
        assert!(!is_decoder_bootstrap(&access_unit(&[SPS, PPS, P_SLICE])).unwrap());
        assert!(!is_decoder_bootstrap(&access_unit(&[SPS, IDR])).unwrap());
        assert!(!is_decoder_bootstrap(&access_unit(&[PPS, IDR])).unwrap());
        assert!(!is_decoder_bootstrap(IDR).unwrap());
    }

    #[test]
    fn parameter_sets_must_precede_the_first_vcl_slice() {
        assert!(!is_decoder_bootstrap(&access_unit(&[IDR, SPS, PPS])).unwrap());
        assert!(!is_decoder_bootstrap(&access_unit(&[SPS, IDR, PPS])).unwrap());
        assert!(!is_decoder_bootstrap(&access_unit(&[P_SLICE, SPS, PPS, IDR])).unwrap());
        assert!(is_decoder_bootstrap(&access_unit(&[PPS, SPS, PPS, IDR])).unwrap());
    }

    #[test]
    fn accepts_leading_and_trailing_annex_b_zero_bytes() {
        let data = access_unit(&[&[0], SPS, PPS, IDR, &[0, 0]]);
        assert!(is_decoder_bootstrap(&data).unwrap());
    }

    #[test]
    fn rejects_non_annex_b_and_malformed_units() {
        assert!(is_decoder_bootstrap(&[]).is_err());
        assert!(is_decoder_bootstrap(&[0x65, 0x88]).is_err());
        assert!(is_decoder_bootstrap(&[0, 0, 0, 2, 0x65, 0x88]).is_err());
        assert!(is_decoder_bootstrap(&[1, 0, 0, 1, 0x65, 0x88]).is_err());
        assert!(is_decoder_bootstrap(&[0, 0, 1]).is_err());
        assert!(is_decoder_bootstrap(&[0, 0, 1, 0xe7, 0x42]).is_err());
        assert!(is_decoder_bootstrap(&[0, 0, 1, 0x78, 0x01]).is_err());
    }

    #[test]
    fn decoder_gate_rearms_after_a_discontinuity() {
        let bootstrap = access_unit(&[SPS, PPS, IDR]);
        let mut gate = DecoderBootstrapGate::default();

        assert!(!gate.accepts(P_SLICE).unwrap());
        assert!(!gate.accepts(IDR).unwrap());
        assert!(gate.accepts(&bootstrap).unwrap());
        assert!(gate.accepts(P_SLICE).unwrap());

        gate.reset();
        assert!(!gate.accepts(P_SLICE).unwrap());
        assert!(gate.accepts(&bootstrap).unwrap());
    }

    #[test]
    fn malformed_media_never_opens_the_decoder_gate() {
        let mut gate = DecoderBootstrapGate::default();
        assert!(gate.accepts(&[0x65, 0x88]).is_err());
        assert!(!gate.is_synchronized());
    }
}
