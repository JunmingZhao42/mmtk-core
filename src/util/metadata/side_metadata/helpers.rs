use super::SideMetadataSpec;
use crate::util::constants::LOG_BYTES_IN_PAGE;
use crate::util::constants::{BITS_IN_WORD, BYTES_IN_PAGE, LOG_BITS_IN_BYTE};
use crate::util::heap::layout::vm_layout::VMLayout;
use crate::util::memory::MmapStrategy;
#[cfg(target_pointer_width = "32")]
use crate::util::metadata::side_metadata::address_to_chunked_meta_address;
use crate::util::Address;
use crate::MMAPPER;
use std::io::Result;

/// Performs address translation in contiguous metadata spaces (e.g. global and policy-specific in 64-bits, and global in 32-bits)
pub(super) fn address_to_contiguous_meta_address(
    metadata_spec: &SideMetadataSpec,
    data_addr: Address,
) -> Address {
    let log_bits_num = metadata_spec.log_num_of_bits as i32;
    let log_bytes_in_region = metadata_spec.log_bytes_in_region;

    let shift = (LOG_BITS_IN_BYTE as i32) - log_bits_num;

    if shift >= 0 {
        metadata_spec.get_absolute_offset() + ((data_addr >> log_bytes_in_region) >> shift)
    } else {
        metadata_spec.get_absolute_offset() + ((data_addr >> log_bytes_in_region) << (-shift))
    }
}

/// Performs reverse address translation from contiguous metadata bits to data addresses.
///
/// Arguments:
/// * `metadata_spec`: The side metadata spec. It should be contiguous side metadata.
/// * `metadata_addr`; The metadata address. Returned by [`address_to_contiguous_meta_address`].
/// * `bit`: The bit shift for the metadata. Returned by [`meta_byte_lshift`].
pub(super) fn contiguous_meta_address_to_address(
    metadata_spec: &SideMetadataSpec,
    metadata_addr: Address,
    bit: u8,
) -> Address {
    let shift = (LOG_BITS_IN_BYTE as i32) - metadata_spec.log_num_of_bits as i32;
    let relative_meta_addr = metadata_addr - metadata_spec.get_absolute_offset();

    let data_addr_intermediate = if shift >= 0 {
        relative_meta_addr << shift
    } else {
        relative_meta_addr >> (-shift)
    };
    let data_addr_bit_shift = if shift >= 0 {
        metadata_spec.log_bytes_in_region - metadata_spec.log_num_of_bits
    } else {
        metadata_spec.log_bytes_in_region
    };

    let data_addr = (data_addr_intermediate << metadata_spec.log_bytes_in_region)
        + ((bit as usize) << data_addr_bit_shift);

    unsafe { Address::from_usize(data_addr) }
}

/// Unmaps the specified metadata range, or panics.
#[cfg(test)]
pub(crate) fn ensure_munmap_metadata(start: Address, size: usize) {
    use crate::util::memory;
    trace!("ensure_munmap_metadata({}, 0x{:x})", start, size);

    assert!(memory::munmap(start, size).is_ok())
}

/// Unmaps a metadata space (`spec`) for the specified data address range (`start` and `size`)
/// Returns the size in bytes that get munmapped.
#[cfg(test)]
pub(crate) fn ensure_munmap_contiguous_metadata_space(
    start: Address,
    size: usize,
    spec: &SideMetadataSpec,
) -> usize {
    // nearest page-aligned starting address
    let metadata_start = address_to_meta_address(spec, start);
    let mmap_start = metadata_start.align_down(BYTES_IN_PAGE);
    // nearest page-aligned ending address
    let metadata_size = (size + ((1 << addr_rshift(spec)) - 1)) >> addr_rshift(spec);
    let mmap_size = (metadata_start + metadata_size).align_up(BYTES_IN_PAGE) - mmap_start;
    if mmap_size > 0 {
        ensure_munmap_metadata(mmap_start, mmap_size);
    }
    mmap_size
}

/// Tries to mmap the metadata space (`spec`) for the specified data address range (`start` and `size`).
/// Setting `no_reserve` to true means the function will only map address range, without reserving swap-space/physical memory.
/// Returns the size in bytes that gets mmapped in the function if success.
pub(super) fn try_mmap_contiguous_metadata_space(
    start: Address,
    size: usize,
    spec: &SideMetadataSpec,
    no_reserve: bool,
) -> Result<usize> {
    debug_assert!(start.is_aligned_to(BYTES_IN_PAGE));
    debug_assert!(size % BYTES_IN_PAGE == 0);

    // nearest page-aligned starting address
    let metadata_start = address_to_meta_address(spec, start);
    let mmap_start = metadata_start.align_down(BYTES_IN_PAGE);
    // nearest page-aligned ending address
    let metadata_size = (size + ((1 << addr_rshift(spec)) - 1)) >> addr_rshift(spec);
    let mmap_size = (metadata_start + metadata_size).align_up(BYTES_IN_PAGE) - mmap_start;
    if mmap_size > 0 {
        if !no_reserve {
            MMAPPER.ensure_mapped(
                mmap_start,
                mmap_size >> LOG_BYTES_IN_PAGE,
                MmapStrategy::SIDE_METADATA,
            )
        } else {
            MMAPPER.quarantine_address_range(
                mmap_start,
                mmap_size >> LOG_BYTES_IN_PAGE,
                MmapStrategy::SIDE_METADATA,
            )
        }
        .map(|_| mmap_size)
    } else {
        Ok(0)
    }
}

/// Performs the translation of data address (`data_addr`) to metadata address for the specified metadata (`metadata_spec`).
pub(crate) fn address_to_meta_address(
    metadata_spec: &SideMetadataSpec,
    data_addr: Address,
) -> Address {
    #[cfg(target_pointer_width = "32")]
    let res = {
        if metadata_spec.is_global {
            address_to_contiguous_meta_address(metadata_spec, data_addr)
        } else {
            address_to_chunked_meta_address(metadata_spec, data_addr)
        }
    };
    #[cfg(target_pointer_width = "64")]
    let res = { address_to_contiguous_meta_address(metadata_spec, data_addr) };

    trace!(
        "address_to_meta_address({:?}, addr: {}) -> 0x{:x}",
        metadata_spec,
        data_addr,
        res
    );

    res
}

pub(super) const fn addr_rshift(metadata_spec: &SideMetadataSpec) -> i32 {
    ((LOG_BITS_IN_BYTE as usize) + metadata_spec.log_bytes_in_region
        - (metadata_spec.log_num_of_bits)) as i32
}

#[allow(dead_code)]
pub(super) const fn metadata_address_range_size(metadata_spec: &SideMetadataSpec) -> usize {
    1usize << (VMLayout::LOG_ARCH_ADDRESS_SPACE - addr_rshift(metadata_spec) as usize)
}

pub(super) fn meta_byte_lshift(metadata_spec: &SideMetadataSpec, data_addr: Address) -> u8 {
    let bits_num_log = metadata_spec.log_num_of_bits as i32;
    if bits_num_log >= 3 {
        return 0;
    }
    let rem_shift = BITS_IN_WORD as i32 - ((LOG_BITS_IN_BYTE as i32) - bits_num_log);
    ((((data_addr >> metadata_spec.log_bytes_in_region) << rem_shift) >> rem_shift) << bits_num_log)
        as u8
}

pub(super) fn meta_byte_mask(metadata_spec: &SideMetadataSpec) -> u8 {
    let bits_num_log = metadata_spec.log_num_of_bits;
    ((1usize << (1usize << bits_num_log)) - 1) as u8
}

/// The result type for find meta bits functions.
pub enum FindMetaBitResult {
    Found { addr: Address, bit: u8 },
    NotFound,
    UnmappedMetadata,
}

// Check and find the last bit that is set. We try load words where possible, and fall back to load bytes.
pub fn find_last_non_zero_bit_in_metadata_bytes(
    meta_start: Address,
    meta_end: Address,
) -> FindMetaBitResult {
    use crate::util::constants::BYTES_IN_ADDRESS;
    use crate::util::heap::vm_layout::MMAP_CHUNK_BYTES;

    let mut cur = meta_end;
    // We need to check if metadata address is mapped or not. But we only check at chunk granularity.
    // This records the start of a chunk that is tested to be mapped.
    let mut mapped_chunk = Address::MAX;
    while cur > meta_start {
        // If we can check the whole word, set step to word size. Otherwise, the step is 1 (byte) and we check byte.
        let step = if cur.is_aligned_to(BYTES_IN_ADDRESS)
            && cur.align_down(BYTES_IN_ADDRESS) >= meta_start
        {
            BYTES_IN_ADDRESS
        } else {
            1
        };
        // Move to the address so we can load from it
        cur -= step;

        // If we are looking at an address that is not in a mapped chunk, we need to check if the chunk if mapped.
        if cur < mapped_chunk {
            if cur.is_mapped() {
                // This is mapped. No need to check for this chunk.
                mapped_chunk = cur.align_down(MMAP_CHUNK_BYTES);
            } else {
                return FindMetaBitResult::UnmappedMetadata;
            }
        }

        if step == BYTES_IN_ADDRESS {
            // Load and check a usize word
            let value = unsafe { cur.load::<usize>() };
            if value != 0 {
                // Find the exact non-zero byte within the usize using bitwise operations
                let byte_offset = (value.trailing_zeros() / 8) as usize;
                let byte_addr = cur + byte_offset;
                let byte_value: u8 = ((value >> (byte_offset * 8)) & 0xFF) as u8;
                let bit = find_last_non_zero_bit_in_u8(byte_value).unwrap();
                return FindMetaBitResult::Found {
                    addr: byte_addr,
                    bit,
                };
            }
        } else {
            // Load and check a byte
            let value = unsafe { cur.load::<u8>() };
            if let Some(bit) = find_last_non_zero_bit_in_u8(value) {
                return FindMetaBitResult::Found { addr: cur, bit };
            }
        }
    }
    FindMetaBitResult::NotFound
}

// Check and find the last non-zero bit in the same byte.
pub fn find_last_non_zero_bit_in_metadata_bits(
    addr: Address,
    start_bit: u8,
    end_bit: u8,
) -> FindMetaBitResult {
    if !addr.is_mapped() {
        return FindMetaBitResult::UnmappedMetadata;
    }
    let byte = unsafe { addr.load::<u8>() };
    if let Some(bit) = find_last_non_zero_bit_in_u8(byte) {
        if bit >= start_bit && bit < end_bit {
            return FindMetaBitResult::Found { addr, bit };
        }
    }
    FindMetaBitResult::NotFound
}

fn find_last_non_zero_bit_in_u8(byte_value: u8) -> Option<u8> {
    if byte_value != 0 {
        let bit = byte_value.trailing_zeros();
        debug_assert!(bit < 8);
        Some(bit as u8)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::metadata::side_metadata::*;

    fn test_round_trip_conversion(spec: &SideMetadataSpec, test_data: &[Address]) {
        for ref_addr in test_data {
            let addr = *ref_addr;

            // This is an aligned address. When we do roundtrip conversion, we will get back the original address.
            {
                assert!(addr.is_aligned_to(1 << spec.log_bytes_in_region));
                let meta_addr = address_to_contiguous_meta_address(spec, addr);
                let shift = meta_byte_lshift(spec, addr);
                assert_eq!(
                    contiguous_meta_address_to_address(spec, meta_addr, shift),
                    addr
                );
            }

            // This is an unaligned address. When we do roundtrip conversion, we will get the aligned address.
            {
                let next_addr = addr + 1usize;
                let meta_addr = address_to_contiguous_meta_address(spec, next_addr);
                let shift = meta_byte_lshift(spec, next_addr);
                assert_eq!(
                    contiguous_meta_address_to_address(spec, meta_addr, shift),
                    addr
                ); // we get back addr (which is the aligned address)
            }
        }
    }

    const TEST_ADDRESS_8B_REGION: [Address; 8] = [
        unsafe { Address::from_usize(0x8000_0000) },
        unsafe { Address::from_usize(0x8000_0008) },
        unsafe { Address::from_usize(0x8000_0010) },
        unsafe { Address::from_usize(0x8000_0018) },
        unsafe { Address::from_usize(0x8000_0020) },
        unsafe { Address::from_usize(0x8001_0000) },
        unsafe { Address::from_usize(0x8001_0008) },
        unsafe { Address::from_usize(0xd000_0000) },
    ];

    #[test]
    fn test_contiguous_metadata_conversion_0_3() {
        let spec = SideMetadataSpec {
            name: "ContiguousMetadataTestSpec",
            is_global: true,
            offset: SideMetadataOffset::addr(GLOBAL_SIDE_METADATA_BASE_ADDRESS),
            log_num_of_bits: 0,
            log_bytes_in_region: 3,
        };

        test_round_trip_conversion(&spec, &TEST_ADDRESS_8B_REGION);
    }

    #[test]
    fn test_contiguous_metadata_conversion_1_3() {
        let spec = SideMetadataSpec {
            name: "ContiguousMetadataTestSpec",
            is_global: true,
            offset: SideMetadataOffset::addr(GLOBAL_SIDE_METADATA_BASE_ADDRESS),
            log_num_of_bits: 1,
            log_bytes_in_region: 3,
        };

        test_round_trip_conversion(&spec, &TEST_ADDRESS_8B_REGION);
    }

    #[test]
    fn test_contiguous_metadata_conversion_4_3() {
        let spec = SideMetadataSpec {
            name: "ContiguousMetadataTestSpec",
            is_global: true,
            offset: SideMetadataOffset::addr(GLOBAL_SIDE_METADATA_BASE_ADDRESS),
            log_num_of_bits: 4,
            log_bytes_in_region: 3,
        };

        test_round_trip_conversion(&spec, &TEST_ADDRESS_8B_REGION);
    }

    #[test]
    fn test_contiguous_metadata_conversion_5_3() {
        let spec = SideMetadataSpec {
            name: "ContiguousMetadataTestSpec",
            is_global: true,
            offset: SideMetadataOffset::addr(GLOBAL_SIDE_METADATA_BASE_ADDRESS),
            log_num_of_bits: 5,
            log_bytes_in_region: 3,
        };

        test_round_trip_conversion(&spec, &TEST_ADDRESS_8B_REGION);
    }

    const TEST_ADDRESS_4KB_REGION: [Address; 8] = [
        unsafe { Address::from_usize(0x8000_0000) },
        unsafe { Address::from_usize(0x8000_1000) },
        unsafe { Address::from_usize(0x8000_2000) },
        unsafe { Address::from_usize(0x8000_3000) },
        unsafe { Address::from_usize(0x8000_4000) },
        unsafe { Address::from_usize(0x8001_0000) },
        unsafe { Address::from_usize(0x8001_1000) },
        unsafe { Address::from_usize(0xd000_0000) },
    ];

    #[test]
    fn test_contiguous_metadata_conversion_0_12() {
        let spec = SideMetadataSpec {
            name: "ContiguousMetadataTestSpec",
            is_global: true,
            offset: SideMetadataOffset::addr(GLOBAL_SIDE_METADATA_BASE_ADDRESS),
            log_num_of_bits: 0,
            log_bytes_in_region: 12, // 4K
        };

        test_round_trip_conversion(&spec, &TEST_ADDRESS_4KB_REGION);
    }
}
