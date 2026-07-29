//! Polar Measurement Data (PMD) service: UUIDs, control-point commands and
//! frame decoding for the H10's high-resolution raw streams.
//!
//! Protocol reference: the MIT-licensed official Polar BLE SDK
//! (polarofficial/polar-ble-sdk) technical documentation, cross-checked
//! against community reverse-engineering notes. The H10 timestamps each
//! sample in nanoseconds since the Polar epoch 2000-01-01T00:00:00Z; the
//! value in a data frame is the time of the LAST sample in that frame.

use uuid::Uuid;

pub const PMD_SERVICE: Uuid = Uuid::from_u128(0xfb005c80_02e7_f387_1cad_8acd2d8df0c8);
pub const PMD_CONTROL: Uuid = Uuid::from_u128(0xfb005c81_02e7_f387_1cad_8acd2d8df0c8);
pub const PMD_DATA: Uuid = Uuid::from_u128(0xfb005c82_02e7_f387_1cad_8acd2d8df0c8);

/// PMD measurement type identifiers (Polar SDK PmdMeasurementType).
pub const MTYPE_ECG: u8 = 0x00;
pub const MTYPE_ACC: u8 = 0x02;

/// Nanoseconds between two ECG samples at 130 Hz.
pub const ECG_PERIOD_NS: u64 = 1_000_000_000 / 130;

/// Difference in nanoseconds between the Polar epoch (2000-01-01) and the
/// Unix epoch (1970-01-01), so device timestamps can be mapped to Unix time.
pub const POLAR_EPOCH_OFFSET_NS: u64 = 946_684_800_000_000_000;

/// Start ECG streaming at 130 Hz, 14-bit resolution.
/// `0x02` = request-measurement-start, `0x00` = ECG, then setting TLVs:
/// SAMPLE_RATE(0x00)=130, RESOLUTION(0x01)=14.
pub fn start_ecg_cmd() -> Vec<u8> {
    vec![
        0x02, MTYPE_ECG, //
        0x00, 0x01, 0x82, 0x00, // sample rate = 130 Hz (0x0082, LE)
        0x01, 0x01, 0x0e, 0x00, // resolution = 14 bit (0x000e, LE)
    ]
}

/// Stop a running measurement of the given type. `0x03` = stop-measurement.
pub fn stop_cmd(mtype: u8) -> Vec<u8> {
    vec![0x03, mtype]
}

#[derive(Debug)]
pub struct EcgFrame {
    /// Device timestamp (ns since Polar epoch) of the last sample in the frame.
    pub timestamp_ns: u64,
    /// One microvolt value per sample, oldest first.
    pub samples_uv: Vec<i32>,
}

/// Decode an uncompressed ECG PMD data frame (frame type 0x00).
///
/// Layout: [type:1][timestamp:8 LE][frame_type:1][samples...], each sample a
/// 24-bit signed little-endian microvolt value.
pub fn parse_ecg(data: &[u8]) -> Option<EcgFrame> {
    if data.len() < 10 || data[0] != MTYPE_ECG {
        return None;
    }
    let timestamp_ns = u64::from_le_bytes(data[1..9].try_into().ok()?);
    let frame_type = data[9];
    if frame_type != 0x00 {
        // Only the uncompressed ECG frame type is handled here.
        return None;
    }
    let body = &data[10..];
    let mut samples_uv = Vec::with_capacity(body.len() / 3);
    for chunk in body.chunks_exact(3) {
        let sign = if chunk[2] & 0x80 != 0 { 0xff } else { 0x00 };
        let val = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], sign]);
        samples_uv.push(val);
    }
    Some(EcgFrame {
        timestamp_ns,
        samples_uv,
    })
}

/// Nanoseconds between two accelerometer samples at 200 Hz.
pub const ACC_PERIOD_NS: u64 = 1_000_000_000 / 200;

/// Start accelerometer streaming at 200 Hz, 16-bit, +/-8 g.
pub fn start_acc_cmd() -> Vec<u8> {
    vec![
        0x02, MTYPE_ACC, //
        0x00, 0x01, 0xc8, 0x00, // sample rate = 200 Hz
        0x01, 0x01, 0x10, 0x00, // resolution = 16 bit
        0x02, 0x01, 0x08, 0x00, // range = +/-8 g
    ]
}

#[derive(Debug)]
pub struct AccFrame {
    pub timestamp_ns: u64,
    /// One [x, y, z] triple per sample, in milli-g.
    pub samples_mg: Vec<[i32; 3]>,
}

/// Decode an accelerometer PMD data frame. Handles both the uncompressed
/// layout (consecutive int16 LE X/Y/Z triples) and Polar's delta-compressed
/// frame (a 16-bit reference sample followed by bit-packed signed deltas).
/// The frame-type byte's high bit marks a compressed frame.
pub fn parse_acc(data: &[u8]) -> Option<AccFrame> {
    if data.len() < 10 || data[0] != MTYPE_ACC {
        return None;
    }
    let timestamp_ns = u64::from_le_bytes(data[1..9].try_into().ok()?);
    let frame_type = data[9];
    let body = &data[10..];
    let channels = 3usize;
    let compressed = frame_type & 0x80 != 0;
    let mut samples_mg: Vec<[i32; 3]> = Vec::new();

    if !compressed {
        for chunk in body.chunks_exact(2 * channels) {
            samples_mg.push([
                i16::from_le_bytes([chunk[0], chunk[1]]) as i32,
                i16::from_le_bytes([chunk[2], chunk[3]]) as i32,
                i16::from_le_bytes([chunk[4], chunk[5]]) as i32,
            ]);
        }
        return Some(AccFrame {
            timestamp_ns,
            samples_mg,
        });
    }

    if body.len() < channels * 2 {
        return None;
    }
    let mut cur = [
        i16::from_le_bytes([body[0], body[1]]) as i32,
        i16::from_le_bytes([body[2], body[3]]) as i32,
        i16::from_le_bytes([body[4], body[5]]) as i32,
    ];
    samples_mg.push(cur);
    let mut pos = channels * 2;
    while pos + 2 <= body.len() {
        let delta_size = body[pos] as usize;
        let sample_count = body[pos + 1] as usize;
        pos += 2;
        if delta_size == 0 || sample_count == 0 {
            break;
        }
        let total = sample_count * channels;
        let mut bit = 0usize;
        let base_bit = pos * 8;
        for v in 0..total {
            let raw = read_bits_le(body, base_bit + bit, delta_size)?;
            cur[v % channels] += sign_extend(raw, delta_size);
            if v % channels == channels - 1 {
                samples_mg.push(cur);
            }
            bit += delta_size;
        }
        pos += bit.div_ceil(8);
    }
    Some(AccFrame {
        timestamp_ns,
        samples_mg,
    })
}

/// Read `n` bits (LSB-first) from a byte slice starting at an absolute bit
/// offset.
fn read_bits_le(data: &[u8], start_bit: usize, n: usize) -> Option<u32> {
    let mut val: u32 = 0;
    for i in 0..n {
        let bit_pos = start_bit + i;
        let byte = data.get(bit_pos / 8)?;
        val |= (((byte >> (bit_pos % 8)) & 1) as u32) << i;
    }
    Some(val)
}

/// Sign-extend an `n`-bit two's-complement value held in a u32 to i32.
fn sign_extend(val: u32, bits: usize) -> i32 {
    if bits == 0 || bits >= 32 {
        return val as i32;
    }
    let shift = 32 - bits;
    ((val << shift) as i32) >> shift
}

