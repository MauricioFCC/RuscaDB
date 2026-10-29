#![allow(missing_docs)]
#![cfg(feature = "fastlanes")]
use ruscadb::FastLanes;

#[test] // @spec AC-0176-01
fn test_ac_0176_01_pack() {
    let lanes = FastLanes::new();
    let values: Vec<u32> = (0..100).collect();
    let bits = FastLanes::bit_width(99);
    let packed = lanes.pack(&values, bits);
    assert_eq!(packed.len(), FastLanes::packed_len(values.len(), bits));
    assert_eq!(packed.len(), 88);
}

#[test] // @spec AC-0176-02
fn test_ac_0176_02_unpack() {
    let lanes = FastLanes::new();
    for bits in [1u32, 3, 7, 13, 31, 32] {
        let mask = if bits == 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        let values: Vec<u32> = (0..64u32)
            .map(|i| i.wrapping_mul(2_654_435_761) & mask)
            .collect();
        let packed = lanes.pack(&values, bits);
        assert_eq!(lanes.unpack(&packed, values.len(), bits), values);
    }
}
