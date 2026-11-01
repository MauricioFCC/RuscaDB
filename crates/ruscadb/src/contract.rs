//! Fingerprint del contrato publico (SPEC-0187, P2.3).
//!
//! Permite fijar por hash del ABI/superficie publica en vez de solo semver.
//! El hash cubre las lineas `pub use` de `lib.rs` y la version de ABI.

/// Fingerprint FNV-1a 64 del contrato publico.
///
/// Returns:
///     Hash estable para un mismo `lib.rs`; cambia si cambia la superficie.
pub fn fingerprint() -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in include_str!("lib.rs").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Fingerprint en hexadecimal (16 caracteres).
///
/// Returns:
///     `format!("{:016x}", fingerprint())`.
pub fn fingerprint_hex() -> String {
    format!("{:016x}", fingerprint())
}
