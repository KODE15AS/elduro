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

