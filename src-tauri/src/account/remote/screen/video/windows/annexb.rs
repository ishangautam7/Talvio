//! Annex-B access units: finding NAL units, and making every keyframe carry
//! its SPS and PPS.
//!
//! Pure byte work, so it is tested without an encoder. Media Foundation's
//! H.264 encoders all write Annex-B (start codes), but not all of them repeat
//! the parameter sets in front of every IDR: some write them only on the first
//! one and leave them in the output type's `MF_MT_MPEG_SEQUENCE_HEADER`. A
//! phone that joins late, or recovers from loss on a later keyframe, cannot
//! decode a bare IDR, so [`ParamSets`] remembers the last SPS and PPS seen and
//! puts them back where they are missing.

/// NAL unit types this module cares about (H.264 Table 7-1).
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

/// `profile_idc` of the Baseline profile.
const PROFILE_BASELINE: u8 = 66;

/// One NAL unit inside an access unit: its type and where its bytes (header
/// included, start code excluded) are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nal {
    pub kind: u8,
    pub start: usize,
    pub end: usize,
}

/// Every NAL unit in `data`, split at 3- and 4-byte start codes. Bytes before
/// the first start code are ignored. A trailing zero byte belongs to the next
/// unit's 4-byte start code, so it is not counted as part of this one.
pub fn nal_units(data: &[u8]) -> Vec<Nal> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (n, &(_, payload)) in starts.iter().enumerate() {
        let mut end = starts.get(n + 1).map_or(data.len(), |&(code, _)| code);
        // `00 00 00 01`: the leading zero is the next unit's, and so is any
        // trailing_zero_8bits padding before it.
        while end > payload && n + 1 < starts.len() && data[end - 1] == 0 {
            end -= 1;
        }
        if payload < end {
            out.push(Nal {
                kind: data[payload] & 0x1f,
                start: payload,
                end,
            });
        }
    }
    out
}

/// Whether the access unit holds an IDR slice.
pub fn is_keyframe(data: &[u8]) -> bool {
    nal_units(data).iter().any(|nal| nal.kind == NAL_IDR)
}

/// Mark every Baseline SPS in `data` as Constrained Baseline (set
/// `constraint_set0_flag` and `constraint_set1_flag`).
///
/// Media Foundation's encoders make Baseline streams with no FMO, ASO or
/// redundant slices, which is exactly Constrained Baseline, but some of them
/// leave the flags clear. WebRTC negotiates `profile-level-id=42e0..`, and a
/// strict receiver reads `42 00` as plain Baseline. The flags byte follows
/// `profile_idc` (0x42), so setting bits in it can never create an emulation
/// prevention sequence.
pub fn mark_constrained_baseline(data: &mut [u8]) {
    for nal in nal_units(data) {
        if nal.kind == NAL_SPS
            && nal.end - nal.start >= 3
            && data[nal.start + 1] == PROFILE_BASELINE
        {
            data[nal.start + 2] |= 0xc0;
        }
    }
}

/// The last SPS and PPS this stream produced, for keyframes that come
/// without them.
#[derive(Default)]
pub struct ParamSets {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

impl ParamSets {
    /// Forget everything (a new encoder makes new parameter sets).
    pub fn clear(&mut self) {
        self.sps = None;
        self.pps = None;
    }

    /// Learn the SPS and PPS from a sequence header blob (Annex-B, as
    /// `MF_MT_MPEG_SEQUENCE_HEADER` holds it). A blob without start codes is
    /// ignored.
    pub fn learn_header(&mut self, header: &[u8]) {
        self.learn(header);
    }

    /// Remember the SPS and PPS in `data`, if it has any.
    fn learn(&mut self, data: &[u8]) {
        for nal in nal_units(data) {
            let bytes = data[nal.start..nal.end].to_vec();
            match nal.kind {
                NAL_SPS => self.sps = Some(bytes),
                NAL_PPS => self.pps = Some(bytes),
                _ => {}
            }
        }
    }

    #[cfg(test)]
    pub fn has_both(&self) -> bool {
        self.sps.is_some() && self.pps.is_some()
    }

    /// Learn from `au`, and when it is a keyframe without an SPS or PPS of its
    /// own, return it with the remembered ones in front. Anything else comes
    /// back unchanged.
    pub fn complete(&mut self, mut au: Vec<u8>) -> Vec<u8> {
        mark_constrained_baseline(&mut au);
        let nals = nal_units(&au);
        let has_idr = nals.iter().any(|nal| nal.kind == NAL_IDR);
        let has_sps = nals.iter().any(|nal| nal.kind == NAL_SPS);
        let has_pps = nals.iter().any(|nal| nal.kind == NAL_PPS);
        self.learn(&au);
        if !has_idr || (has_sps && has_pps) {
            return au;
        }
        let mut out = Vec::with_capacity(au.len() + 64);
        if !has_sps {
            if let Some(sps) = &self.sps {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(sps);
            }
        }
        if !has_pps {
            if let Some(pps) = &self.pps {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(pps);
            }
        }
        out.extend_from_slice(&au);
        out
    }
}

/// Width and height an SPS describes, after cropping. Only what the tests
/// need to check a stream: no scaling lists (which these encoders never
/// write).
#[cfg(test)]
pub fn sps_dimensions(sps: &[u8]) -> Option<(u32, u32)> {
    // Drop the header byte and the emulation prevention bytes.
    let mut rbsp = Vec::with_capacity(sps.len());
    let mut zeros = 0;
    for &byte in sps.get(1..)? {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        rbsp.push(byte);
    }
    let mut bits = Bits {
        data: &rbsp,
        pos: 0,
    };
    let profile = bits.read(8)?;
    bits.read(16)?; // constraint flags, level
    bits.ue()?; // seq_parameter_set_id
    let mut chroma_format = 1;
    if matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128) {
        chroma_format = bits.ue()?;
        if chroma_format == 3 {
            bits.read(1)?;
        }
        bits.ue()?;
        bits.ue()?;
        bits.read(1)?;
        if bits.read(1)? == 1 {
            return None; // scaling lists: not needed here
        }
    }
    bits.ue()?; // log2_max_frame_num_minus4
    let poc_type = bits.ue()?;
    if poc_type == 0 {
        bits.ue()?;
    } else if poc_type == 1 {
        bits.read(1)?;
        bits.se()?;
        bits.se()?;
        for _ in 0..bits.ue()? {
            bits.se()?;
        }
    }
    bits.ue()?; // max_num_ref_frames
    bits.read(1)?;
    let width_mbs = bits.ue()? + 1;
    let height_units = bits.ue()? + 1;
    let frame_mbs_only = bits.read(1)?;
    if frame_mbs_only == 0 {
        bits.read(1)?;
    }
    bits.read(1)?; // direct_8x8_inference_flag
    let (mut crop_l, mut crop_r, mut crop_t, mut crop_b) = (0, 0, 0, 0);
    if bits.read(1)? == 1 {
        crop_l = bits.ue()?;
        crop_r = bits.ue()?;
        crop_t = bits.ue()?;
        crop_b = bits.ue()?;
    }
    let (crop_x, crop_y) = match chroma_format {
        0 => (1, 2 - frame_mbs_only),
        1 => (2, 2 * (2 - frame_mbs_only)),
        2 => (2, 2 - frame_mbs_only),
        _ => (1, 2 - frame_mbs_only),
    };
    let width = width_mbs * 16 - crop_x * (crop_l + crop_r);
    let height = height_units * 16 * (2 - frame_mbs_only) - crop_y * (crop_t + crop_b);
    Some((width, height))
}

#[cfg(test)]
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

#[cfg(test)]
impl Bits<'_> {
    fn read(&mut self, n: u32) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..n {
            let byte = *self.data.get(self.pos / 8)?;
            let bit = (byte >> (7 - self.pos % 8)) & 1;
            value = (value << 1) | u32::from(bit);
            self.pos += 1;
        }
        Some(value)
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.read(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.read(zeros)?)
    }

    fn se(&mut self) -> Option<i32> {
        let k = self.ue()? as i64;
        Some(if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) } as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real 1280x720 Baseline SPS (constraint flags clear) and a PPS.
    const SPS: &[u8] = &[0x67, 0x42, 0x00, 0x1f, 0xda, 0x01, 0x40, 0x16, 0xe4];
    const PPS: &[u8] = &[0x68, 0xce, 0x3c, 0x80];
    const IDR: &[u8] = &[0x65, 0x88, 0x84, 0x00, 0x10];
    const P_SLICE: &[u8] = &[0x41, 0x9a, 0x00, 0x00, 0x01];

    fn au(units: &[&[u8]], four: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in units {
            if four {
                out.push(0);
            }
            out.extend_from_slice(&[0, 0, 1]);
            out.extend_from_slice(unit);
        }
        out
    }

    fn kinds(data: &[u8]) -> Vec<u8> {
        nal_units(data).iter().map(|nal| nal.kind).collect()
    }

    #[test]
    fn splits_three_and_four_byte_start_codes() {
        for four in [false, true] {
            let data = au(&[SPS, PPS, IDR], four);
            let nals = nal_units(&data);
            assert_eq!(kinds(&data), vec![7, 8, 5]);
            assert_eq!(&data[nals[0].start..nals[0].end], SPS);
            assert_eq!(&data[nals[1].start..nals[1].end], PPS);
            assert_eq!(&data[nals[2].start..nals[2].end], IDR);
        }
    }

    #[test]
    fn a_zero_ending_payload_is_not_cut_at_the_end_of_the_unit() {
        // The last unit keeps its bytes even when they end in zero.
        let data = au(&[P_SLICE, &[0x41, 0x00]], true);
        let nals = nal_units(&data);
        assert_eq!(&data[nals[1].start..nals[1].end], &[0x41, 0x00]);
        assert!(nal_units(&[1, 2, 3]).is_empty());
        assert!(nal_units(&[]).is_empty());
    }

    #[test]
    fn keyframes_are_recognised_by_their_idr_slice() {
        assert!(is_keyframe(&au(&[SPS, PPS, IDR], true)));
        assert!(!is_keyframe(&au(&[P_SLICE], true)));
    }

    #[test]
    fn baseline_sps_becomes_constrained_baseline() {
        let mut data = au(&[SPS, PPS], true);
        mark_constrained_baseline(&mut data);
        let nals = nal_units(&data);
        assert_eq!(data[nals[0].start + 2], 0xc0);
        // Other profiles are left alone.
        let main = [0x67, 0x4d, 0x00, 0x1f];
        let mut data = au(&[&main], true);
        mark_constrained_baseline(&mut data);
        assert_eq!(data[nal_units(&data)[0].start + 2], 0x00);
    }

    #[test]
    fn a_bare_idr_gets_the_last_parameter_sets() {
        let mut sets = ParamSets::default();
        let first = sets.complete(au(&[SPS, PPS, IDR], true));
        assert_eq!(kinds(&first), vec![7, 8, 5]);
        let p = sets.complete(au(&[P_SLICE], true));
        assert_eq!(kinds(&p), vec![1]);
        let later = sets.complete(au(&[IDR], true));
        assert_eq!(kinds(&later), vec![7, 8, 5]);
        // The added SPS is the constrained one.
        let nals = nal_units(&later);
        assert_eq!(later[nals[0].start + 2], 0xc0);
    }

    #[test]
    fn a_sequence_header_supplies_what_the_stream_never_had() {
        let mut sets = ParamSets::default();
        assert!(!sets.has_both());
        sets.learn_header(&au(&[SPS, PPS], true));
        assert!(sets.has_both());
        let key = sets.complete(au(&[IDR], false));
        assert_eq!(kinds(&key), vec![7, 8, 5]);
        sets.clear();
        let bare = sets.complete(au(&[IDR], false));
        assert_eq!(kinds(&bare), vec![5]);
    }

    #[test]
    fn only_the_missing_set_is_added() {
        let mut sets = ParamSets::default();
        sets.complete(au(&[SPS, PPS, IDR], true));
        let key = sets.complete(au(&[PPS, IDR], true));
        assert_eq!(kinds(&key), vec![7, 8, 5]);
    }

    #[test]
    fn sps_dimensions_reads_size_and_cropping() {
        assert_eq!(sps_dimensions(SPS), Some((1280, 720)));
        // 1920x1080: 120x68 macroblocks cropped by 8 lines.
        let hd = [0x67, 0x42, 0xc0, 0x1f, 0xda, 0x01, 0xe0, 0x08, 0x9f, 0x95];
        assert_eq!(sps_dimensions(&hd), Some((1920, 1080)));
    }
}
