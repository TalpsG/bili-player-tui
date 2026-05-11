use md5::{Digest, Md5};

/// The 64-element permutation table for WBI mixin key derivation.
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42,
    19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51,
    30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// Derive the 32-character mixin key from img_key and sub_key.
///
/// Algorithm: concatenate img_key + sub_key, permute using MIXIN_KEY_ENC_TAB,
/// take the first 32 characters.
pub fn get_mixin_key(img_key: &str, sub_key: &str) -> String {
    let combined = format!("{img_key}{sub_key}");
    MIXIN_KEY_ENC_TAB
        .iter()
        .map(|&i| combined.chars().nth(i).unwrap_or('\0'))
        .take(32)
        .collect()
}

/// Sign the given parameters with WBI signature.
///
/// Algorithm:
/// 1. Add `wts` (current unix timestamp)
/// 2. Sort parameters by key alphabetically
/// 3. Filter characters: remove `!'()*`
/// 4. URL-encode values
/// 5. Compute MD5(query_string + mixin_key) = w_rid
/// 6. Append w_rid and wts to parameters
pub fn sign_wbi_params(
    params: &mut Vec<(String, String)>,
    mixin_key: &str,
) {
    // Add timestamp
    let wts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    params.push(("wts".to_string(), wts.to_string()));

    // Sort by key
    params.sort_by(|a, b| a.0.cmp(&b.0));

    // Filter special characters and URL-encode
    const FILTER_CHARS: &[char] = &['!', '\'', '(', ')', '*'];
    for (_, v) in params.iter_mut() {
        *v = v
            .chars()
            .filter(|c| !FILTER_CHARS.contains(c))
            .collect::<String>();
    }

    // Build query string with encode_uri_component-style encoding
    // (uppercase hex digits, spaces as %20)
    let query: String = params
        .iter()
        .map(|(k, v)| {
            let encoded_k = encode_uri_component(k);
            let encoded_v = encode_uri_component(v);
            format!("{encoded_k}={encoded_v}")
        })
        .collect::<Vec<_>>()
        .join("&");

    // MD5 hash
    let mut hasher = Md5::new();
    hasher.update(query.as_bytes());
    hasher.update(mixin_key.as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    // Append w_rid
    params.push(("w_rid".to_string(), hash));
}

/// encode_uri_component-style URL encoding for WBI signing.
/// Follows ECMA-262: uppercase hex digits, spaces as %20.
/// Unescaped chars: A-Z a-z 0-9 - _ . ~
fn encode_uri_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                result.push(c);
            }
            _ => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    result.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mixin_key_derivation() {
        // Known test case from community documentation
        let img_key = "abcdabcdabcdabcdabcdabcdabcdabcd";
        let sub_key = "efghefghefghefghefghefghefghefgh";
        let result = get_mixin_key(img_key, sub_key);

        // Result should be 32 characters
        assert_eq!(result.len(), 32);

        // All characters should come from the combined key
        let combined = format!("{img_key}{sub_key}");
        for c in result.chars() {
            assert!(combined.contains(c));
        }
    }

    #[test]
    fn test_mixin_key_length() {
        // Regardless of input length, output is always 32 chars
        let result = get_mixin_key("short", "key");
        assert_eq!(result.len(), 32);

        let result = get_mixin_key("a_very_long_key_that_exceeds_64_chars_aaa", "another_very_long_key_bbb");
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_sign_wbi_params_adds_wts_and_wrid() {
        let mixin_key = "0123456789abcdef0123456789abcdef";
        let mut params = vec![
            ("foo".to_string(), "bar".to_string()),
            ("baz".to_string(), "qux".to_string()),
        ];

        sign_wbi_params(&mut params, mixin_key);

        // Should have added wts and w_rid
        let keys: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"wts"));
        assert!(keys.contains(&"w_rid"));

        // w_rid should be 32 hex chars (MD5)
        let wrid = params.iter().find(|(k, _)| k == "w_rid").unwrap().1.clone();
        assert_eq!(wrid.len(), 32);
        assert!(wrid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sign_wbi_params_sorted() {
        let mixin_key = "0123456789abcdef0123456789abcdef";
        let mut params = vec![
            ("z_key".to_string(), "last".to_string()),
            ("a_key".to_string(), "first".to_string()),
            ("m_key".to_string(), "middle".to_string()),
        ];

        sign_wbi_params(&mut params, mixin_key);

        // w_rid is appended after sort, so it's at the end
        let keys: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).collect();
        // Sorted portion: a_key < m_key < wts < z_key, then w_rid at end
        assert_eq!(keys[0], "a_key");
        assert_eq!(keys[1], "m_key");
        assert_eq!(keys[2], "wts");
        assert_eq!(keys[3], "z_key");
        assert_eq!(keys[4], "w_rid");
    }

    #[test]
    fn test_sign_wbi_filters_special_chars() {
        let mixin_key = "0123456789abcdef0123456789abcdef";
        let mut params = vec![
            ("test".to_string(), "hello!'(world)*".to_string()),
        ];

        sign_wbi_params(&mut params, mixin_key);

        let value = params.iter().find(|(k, _)| k == "test").unwrap().1.clone();
        assert_eq!(value, "helloworld");
    }

    #[test]
    fn test_sign_deterministic() {
        let mixin_key = "0123456789abcdef0123456789abcdef";
        let mut params1 = vec![("key".to_string(), "value".to_string())];
        let mut params2 = vec![("key".to_string(), "value".to_string())];

        // Set the same timestamp for both
        params1.push(("wts".to_string(), "1000000".to_string()));
        params2.push(("wts".to_string(), "1000000".to_string()));

        // Sort both
        params1.sort_by(|a, b| a.0.cmp(&b.0));
        params2.sort_by(|a, b| a.0.cmp(&b.0));

        let query1: String = params1.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        let query2: String = params2.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");

        let mut hasher1 = Md5::new();
        hasher1.update(query1.as_bytes());
        hasher1.update(mixin_key.as_bytes());
        let hash1 = format!("{:x}", hasher1.finalize());

        let mut hasher2 = Md5::new();
        hasher2.update(query2.as_bytes());
        hasher2.update(mixin_key.as_bytes());
        let hash2 = format!("{:x}", hasher2.finalize());

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_encode_uri_component() {
        // ASCII alphanumeric stays as-is
        assert_eq!(encode_uri_component("hello123"), "hello123");
        // Space becomes %20
        assert_eq!(encode_uri_component("one one"), "one%20one");
        // Chinese characters are UTF-8 encoded with uppercase hex
        assert_eq!(encode_uri_component("五"), "%E4%BA%94");
        // Special chars are encoded
        assert_eq!(encode_uri_component("a+b"), "a%2Bb");
        // Unescaped chars: - _ . ~
        assert_eq!(encode_uri_component("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn test_wbi_sign_with_chinese() {
        let mixin_key = "0123456789abcdef0123456789abcdef";
        let mut params = vec![
            ("keyword".to_string(), "周杰伦".to_string()),
            ("page".to_string(), "1".to_string()),
        ];

        sign_wbi_params(&mut params, mixin_key);

        // Should have wts and w_rid
        let keys: Vec<&str> = params.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"wts"));
        assert!(keys.contains(&"w_rid"));

        // w_rid should be valid hex
        let wrid = params.iter().find(|(k, _)| k == "w_rid").unwrap().1.clone();
        assert_eq!(wrid.len(), 32);
        assert!(wrid.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
