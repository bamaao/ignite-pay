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
