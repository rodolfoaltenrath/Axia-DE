//! SDK number captions, including ECMAScript fixed precision rounding.
//! The adapter supplies String(value), preserving its public coercion contract.
pub fn number(
    raw: &str,
    digits: Option<f64>,
    decimal: &str,
    group: &str,
) -> Result<String, &'static str> {
    let fixed;
    let text = if let Some(digits) = digits {
        let digits = if digits.is_nan() { 0. } else { digits.trunc() };
        if !(0. ..=100.).contains(&digits) {
            return Err("locale.invalidDigits");
        }
        let value = raw.parse::<f64>().unwrap_or(f64::NAN);
        if value.is_finite() && value.abs() < 1e21 {
            fixed = to_fixed(value, digits as usize);
            fixed.as_str()
        } else {
            raw
        }
    } else {
        raw
    };
    let mut parts = text.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    let mut out = String::with_capacity(text.len() + 16);
    // Match the legacy /\B(?=(\d{3})+(?!\d))/g even for exponent/string inputs.
    let mut run = 0;
    let mut inserts = Vec::new();
    for (index, c) in whole.char_indices().rev() {
        if c.is_ascii_digit() {
            if run > 0 && run % 3 == 0 {
                inserts.push(index + c.len_utf8());
            }
            run += 1;
        } else {
            if run > 0 && run % 3 == 0 && (c.is_ascii_alphanumeric() || c == '_') {
                inserts.push(index + c.len_utf8());
            }
            run = 0;
        }
    }
    inserts.reverse();
    let mut start = 0;
    for end in inserts {
        out.push_str(&whole[start..end]);
        out.push_str(group);
        start = end;
    }
    out.push_str(&whole[start..]);
    if !fraction.is_empty() {
        out.push_str(decimal);
        out.push_str(fraction);
    }
    Ok(out)
}

fn to_fixed(value: f64, digits: usize) -> String {
    // Exact binary rational * 10^digits. At most 403 significant bits for the
    // SDK range (< 1e21, <= 100 digits); no floating multiplication or ties-to-even.
    let bits = value.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let shift = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    } + digits as i32;
    let mut limbs = vec![mantissa as u32, (mantissa >> 32) as u32];
    for _ in 0..digits {
        let mut carry = 0u64;
        for limb in &mut limbs {
            let n = u64::from(*limb) * 5 + carry;
            *limb = n as u32;
            carry = n >> 32;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
    }
    if shift >= 0 {
        for _ in 0..shift {
            let mut carry = 0;
            for limb in &mut limbs {
                let next = *limb >> 31;
                *limb = (*limb << 1) | carry;
                carry = next;
            }
            if carry != 0 {
                limbs.push(carry);
            }
        }
    } else {
        let mut round = 0;
        for _ in 0..-shift {
            round = limbs[0] & 1;
            let mut carry = 0;
            for limb in limbs.iter_mut().rev() {
                let next = *limb << 31;
                *limb = (*limb >> 1) | carry;
                carry = next;
            }
        }
        if round != 0 {
            let mut carry = true;
            for limb in &mut limbs {
                if !carry {
                    break;
                }
                (*limb, carry) = limb.overflowing_add(1);
            }
            if carry {
                limbs.push(1);
            }
        }
    }
    let mut result = Vec::new();
    while limbs.iter().any(|&n| n != 0) {
        let mut remainder = 0u64;
        for limb in limbs.iter_mut().rev() {
            let n = (remainder << 32) | u64::from(*limb);
            *limb = (n / 10) as u32;
            remainder = n % 10;
        }
        result.push(b'0' + remainder as u8);
    }
    while result.len() <= digits {
        result.push(b'0');
    }
    result.reverse();
    let mut result = String::from_utf8(result).unwrap();
    if digits != 0 {
        result.insert(result.len() - digits, '.');
    }
    if value < 0. {
        result.insert(0, '-');
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_precision_preserves_binary_rounding_ties_sign_and_limits() {
        for (raw, digits, expected) in [
            ("2.5", 0, "3"),
            ("-2.5", 0, "-3"),
            ("1.005", 2, "1.00"),
            ("2.55", 1, "2.5"),
            ("2.35", 1, "2.4"),
            ("0.125", 2, "0.13"),
            ("-0.001", 2, "-0.00"),
            ("-0", 2, "0.00"),
            ("1000000000000000100", 0, "1000000000000000128"),
            ("5e-324", 2, "0.00"),
            ("1e+21", 2, "1e+21"),
            ("NaN", 2, "NaN"),
            ("Infinity", 2, "Infinity"),
        ] {
            assert_eq!(
                number(raw, Some(digits as f64), ".", "").unwrap(),
                expected,
                "{raw}/{digits}"
            );
        }
        assert_eq!(
            number("1", Some(100.), ".", "").unwrap(),
            format!("1.{}", "0".repeat(100))
        );
        assert!(number("1", Some(101.), ".", ",").is_err());
        assert!(number("1", Some(f64::INFINITY), ".", ",").is_err());
        assert_eq!(number("1.5", Some(f64::NAN), ".", "").unwrap(), "2");
    }
    #[test]
    fn separators_do_not_reinterpret_exponents_or_coerced_strings() {
        for (raw, expected) in [
            ("-1234567.80", "-1.234.567,80"),
            ("1234", "1.234"),
            ("1e+21", "1e+21"),
            ("1.234e-7", "1,234e-7"),
            ("NaN", "NaN"),
            ("abc1234", "abc1.234"),
            ("abc123", "abc.123"),
        ] {
            assert_eq!(number(raw, None, ",", ".").unwrap(), expected);
        }
    }
}
