//! Stable monitor identity from EDID (directive §12).
//!
//! Assignments must survive monitors being enumerated in a different order, so a display is
//! identified by what it *is* (manufacturer, model, serial) and only falls back to where it is
//! plugged in (connector) when EDID says too little.

use crate::backend::{EdidIdentity, OutputId};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdidError {
    #[error("EDID is {0} bytes; at least 128 are required")]
    Truncated(usize),
    #[error("EDID does not start with the fixed header")]
    BadHeader,
    #[error("EDID base block checksum is wrong")]
    BadChecksum,
}

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Parse the identity fields of an EDID base block.
pub fn parse_edid(bytes: &[u8]) -> Result<EdidIdentity, EdidError> {
    if bytes.len() < 128 {
        return Err(EdidError::Truncated(bytes.len()));
    }
    if bytes[..8] != HEADER {
        return Err(EdidError::BadHeader);
    }
    if bytes[..128].iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) != 0 {
        return Err(EdidError::BadChecksum);
    }

    // Manufacturer: three 5-bit letters packed big-endian into bytes 8-9 (1 = 'A').
    let packed = u16::from_be_bytes([bytes[8], bytes[9]]);
    let letter = |shift: u16| -> char {
        let value = ((packed >> shift) & 0x1F) as u8;
        if (1..=26).contains(&value) {
            char::from(b'A' + value - 1)
        } else {
            '?'
        }
    };
    let manufacturer: String = [letter(10), letter(5), letter(0)].iter().collect();

    let product_code = u16::from_le_bytes([bytes[10], bytes[11]]);
    let numeric_serial = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);

    let mut text_serial = None;
    let mut model_name = None;
    for offset in [54usize, 72, 90, 108] {
        let d = &bytes[offset..offset + 18];
        // Display descriptors start with three zero bytes; byte 3 is the tag.
        if d[0] == 0 && d[1] == 0 && d[2] == 0 {
            let text = descriptor_text(&d[5..18]);
            match d[3] {
                0xFF if !text.is_empty() => text_serial = Some(text),
                0xFC if !text.is_empty() => model_name = Some(text),
                _ => {}
            }
        }
    }

    let serial = text_serial.or_else(|| (numeric_serial != 0).then(|| numeric_serial.to_string()));
    Ok(EdidIdentity {
        manufacturer,
        product_code,
        serial,
        model_name,
    })
}

/// Descriptor text is ASCII, terminated by a newline and padded with spaces.
fn descriptor_text(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0x0A).unwrap_or(raw.len());
    let text: String = raw[..end]
        .iter()
        .map(|&b| {
            if (0x20..0x7F).contains(&b) {
                char::from(b)
            } else {
                ' '
            }
        })
        .collect();
    text.trim().to_owned()
}

/// Keep IDs to a conservative character set so they are safe as TOML keys, D-Bus strings and
/// file names.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// One present output, as far as identity is concerned.
#[derive(Clone, Debug)]
pub struct IdentityInput {
    pub connector: String,
    pub edid: Option<EdidIdentity>,
}

fn base_id(input: &IdentityInput) -> String {
    match &input.edid {
        Some(edid) => {
            let head = format!(
                "edid:{}-{:04x}",
                sanitize(&edid.manufacturer),
                edid.product_code
            );
            match edid
                .serial
                .as_deref()
                .map(sanitize)
                .filter(|s| !s.is_empty())
            {
                Some(serial) => format!("{head}-{serial}"),
                None => format!("{head}@{}", sanitize(&input.connector)),
            }
        }
        None => format!("conn:{}", sanitize(&input.connector)),
    }
}

/// Compute the stable IDs of all currently present outputs, in input order.
///
/// If two present outputs would get the same ID (identical panels that report identical serials),
/// `@<connector>` is appended to *both*, so the result is deterministic and never ambiguous.
pub fn stable_ids(inputs: &[IdentityInput]) -> Vec<OutputId> {
    let mut ids: Vec<String> = inputs.iter().map(base_id).collect();
    let snapshot = ids.clone();
    for (i, id) in ids.iter_mut().enumerate() {
        let collides = snapshot
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && other == id.as_str());
        if collides && !id.contains('@') {
            id.push('@');
            id.push_str(&sanitize(&inputs[i].connector));
        }
    }
    ids.into_iter().map(OutputId::new).collect()
}

#[cfg(test)]
pub(crate) mod fixtures {
    /// Build a spec-conformant 128-byte EDID 1.4 base block.
    ///
    /// These are synthetic (byte-exact per the EDID layout, valid checksum), not captures from
    /// physical monitors: the development server has none to capture.
    pub fn edid(
        mfg: &str,
        product: u16,
        numeric_serial: u32,
        text_serial: Option<&str>,
        model: Option<&str>,
    ) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        let letters: Vec<u16> = mfg.bytes().map(|c| u16::from(c - b'A' + 1)).collect();
        let packed = (letters[0] << 10) | (letters[1] << 5) | letters[2];
        b[8..10].copy_from_slice(&packed.to_be_bytes());
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&numeric_serial.to_le_bytes());
        b[16] = 10; // week
        b[17] = 30; // year 2020
        b[18] = 1;
        b[19] = 4; // EDID 1.4
        let mut slot = 54;
        let mut descriptor = |tag: u8, text: &str| {
            b[slot + 3] = tag;
            let mut payload = [0x20u8; 13];
            let bytes = text.as_bytes();
            payload[..bytes.len()].copy_from_slice(bytes);
            if bytes.len() < 13 {
                payload[bytes.len()] = 0x0A;
            }
            b[slot + 5..slot + 18].copy_from_slice(&payload);
            slot += 18;
        };
        if let Some(text) = text_serial {
            descriptor(0xFF, text);
        }
        if let Some(text) = model {
            descriptor(0xFC, text);
        }
        let sum = b[..127].iter().fold(0u8, |s, x| s.wrapping_add(*x));
        b[127] = 0u8.wrapping_sub(sum);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::edid;
    use super::*;

    fn input(connector: &str, edid_bytes: Option<Vec<u8>>) -> IdentityInput {
        IdentityInput {
            connector: connector.into(),
            edid: edid_bytes.and_then(|b| parse_edid(&b).ok()),
        }
    }

    #[test]
    fn parses_manufacturer_product_text_serial_and_model() {
        let bytes = edid(
            "DEL",
            0xA0B1,
            0x1234_5678,
            Some("7XJ2K3"),
            Some("DELL U2720Q"),
        );
        let id = parse_edid(&bytes).unwrap();
        assert_eq!(id.manufacturer, "DEL");
        assert_eq!(id.product_code, 0xA0B1);
        assert_eq!(
            id.serial.as_deref(),
            Some("7XJ2K3"),
            "text serial wins over the numeric one"
        );
        assert_eq!(id.model_name.as_deref(), Some("DELL U2720Q"));
    }

    #[test]
    fn falls_back_to_the_numeric_serial() {
        let id = parse_edid(&edid("SAM", 0x0F1C, 16_843_009, None, Some("S24"))).unwrap();
        assert_eq!(id.serial.as_deref(), Some("16843009"));
    }

    #[test]
    fn zero_serial_means_no_serial() {
        let id = parse_edid(&edid("LGD", 0x0612, 0, None, None)).unwrap();
        assert_eq!(id.serial, None);
        assert_eq!(id.model_name, None);
    }

    #[test]
    fn blank_text_serial_is_treated_as_absent() {
        let id = parse_edid(&edid("AUO", 0x1234, 0, Some("   "), None)).unwrap();
        assert_eq!(id.serial, None);
    }

    #[test]
    fn rejects_bad_checksum_bad_header_and_truncation() {
        let mut bad = edid("DEL", 1, 1, None, None);
        bad[20] ^= 0xFF;
        assert_eq!(parse_edid(&bad), Err(EdidError::BadChecksum));

        let mut no_header = edid("DEL", 1, 1, None, None);
        no_header[1] = 0;
        assert_eq!(parse_edid(&no_header), Err(EdidError::BadHeader));

        assert_eq!(
            parse_edid(&edid("DEL", 1, 1, None, None)[..100]),
            Err(EdidError::Truncated(100))
        );
        assert_eq!(parse_edid(&[]), Err(EdidError::Truncated(0)));
    }

    #[test]
    fn extension_blocks_after_the_base_block_are_ignored() {
        let mut bytes = edid("DEL", 0xA0B1, 0, Some("ABC"), None);
        bytes.extend_from_slice(&[0x02; 128]);
        assert!(parse_edid(&bytes).is_ok());
    }

    #[test]
    fn non_letter_manufacturer_codes_do_not_panic() {
        let mut bytes = edid("DEL", 1, 1, None, None);
        bytes[8] = 0;
        bytes[9] = 0;
        let sum = bytes[..127].iter().fold(0u8, |s, x| s.wrapping_add(*x));
        bytes[127] = 0u8.wrapping_sub(sum);
        assert_eq!(parse_edid(&bytes).unwrap().manufacturer, "???");
    }

    #[test]
    fn id_uses_serial_when_present() {
        let ids = stable_ids(&[input(
            "DP-1",
            Some(edid("DEL", 0xA0B1, 0, Some("7XJ2K3"), None)),
        )]);
        assert_eq!(ids[0].as_str(), "edid:DEL-a0b1-7XJ2K3");
    }

    #[test]
    fn id_without_serial_includes_the_connector() {
        let ids = stable_ids(&[input("eDP-1", Some(edid("LGD", 0x0612, 0, None, None)))]);
        assert_eq!(ids[0].as_str(), "edid:LGD-0612@eDP-1");
    }

    #[test]
    fn id_without_edid_falls_back_to_the_connector() {
        let ids = stable_ids(&[input("HDMI-1", None)]);
        assert_eq!(ids[0].as_str(), "conn:HDMI-1");
    }

    #[test]
    fn identical_panels_with_identical_serials_get_disambiguated() {
        let same = edid("DEL", 0xA0B1, 0, Some("SAME"), None);
        let ids = stable_ids(&[input("DP-1", Some(same.clone())), input("DP-2", Some(same))]);
        assert_eq!(ids[0].as_str(), "edid:DEL-a0b1-SAME@DP-1");
        assert_eq!(ids[1].as_str(), "edid:DEL-a0b1-SAME@DP-2");
    }

    #[test]
    fn enumeration_order_does_not_change_ids() {
        let a = input("DP-1", Some(edid("DEL", 0xA0B1, 0, Some("AAA"), None)));
        let b = input("DP-2", Some(edid("SAM", 0x0F1C, 0, Some("BBB"), None)));
        let forward = stable_ids(&[a.clone(), b.clone()]);
        let backward = stable_ids(&[b, a]);
        assert_eq!(forward[0], backward[1]);
        assert_eq!(forward[1], backward[0]);
    }

    #[test]
    fn unusual_characters_are_sanitised() {
        let ids = stable_ids(&[input("weird port/1", None)]);
        assert_eq!(ids[0].as_str(), "conn:weird_port_1");
        let ids = stable_ids(&[input("DP-1", Some(edid("DEL", 1, 0, Some("A B:C"), None)))]);
        assert_eq!(ids[0].as_str(), "edid:DEL-0001-A_B_C");
    }
}
