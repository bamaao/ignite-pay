// Copyright (c) 2026 zouyc zouyccq@gmail.com.
// All rights reserved.
//
// Licensed under the Business Source License 1.1 (BSL 1.1).
// You may not use this file except in compliance with the License.
//
// Change Date: 2031-01-01
// On the Change Date, or the fourth anniversary of the first publicly available
// distribution of the code under the BSL, whichever comes first, the code
// automatically becomes available under the Apache License 2.0.

use solana_program::hash::hashv;

pub fn verify_merkle_proof(
    leaf_hash: &[u8; 32],
    proof: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    let mut current = *leaf_hash;
    for sibling in proof {
        let (left, right) = if &current < sibling {
            (current, *sibling)
        } else {
            (*sibling, current)
        };
        current = hashv(&[&left, &right]).to_bytes();
    }
    current == *root
}
