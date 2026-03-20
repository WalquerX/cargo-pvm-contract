pub fn compute_selector(canonical_signature: &str) -> [u8; 4] {
    let hash = keccak_const::Keccak256::new()
        .update(canonical_signature.as_bytes())
        .finalize();
    [hash[0], hash[1], hash[2], hash[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_selector() {
        let selector = compute_selector("transfer(address,uint256)");
        assert_eq!(selector, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_balance_of_selector() {
        let selector = compute_selector("balanceOf(address)");
        assert_eq!(selector, [0x70, 0xa0, 0x82, 0x31]);
    }

    #[test]
    fn test_total_supply_selector() {
        let selector = compute_selector("totalSupply()");
        assert_eq!(selector, [0x18, 0x16, 0x0d, 0xdd]);
    }

    #[test]
    fn test_mint_selector() {
        let selector = compute_selector("mint(address,uint256)");
        assert_eq!(selector, [0x40, 0xc1, 0x0f, 0x19]);
    }
}
