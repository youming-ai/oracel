#[cfg(test)]
pub fn d(value: &str) -> rust_decimal::Decimal {
    rust_decimal::Decimal::from_str_exact(value).expect("valid decimal")
}
