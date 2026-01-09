use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed, decode_different::DecodeDifferent};
use std::collections::HashSet;

pub fn pallets_in_metadata(meta: &RuntimeMetadataPrefixed) -> HashSet<String> {
    use RuntimeMetadata::*;

    fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
        match s {
            DecodeDifferent::Decoded(v) => v.as_str(),
            DecodeDifferent::Encode(s) => s,
        }
    }

    let mut set = HashSet::new();

    match &meta.1 {
        V9(m) => {
            if let DecodeDifferent::Decoded(modules) = &m.modules {
                for module in modules {
                    set.insert(extract_str(&module.name).to_string());
                }
            }
        }
        V10(m) => {
            if let DecodeDifferent::Decoded(modules) = &m.modules {
                for module in modules {
                    set.insert(extract_str(&module.name).to_string());
                }
            }
        }
        V11(m) => {
            if let DecodeDifferent::Decoded(modules) = &m.modules {
                for module in modules {
                    set.insert(extract_str(&module.name).to_string());
                }
            }
        }
        V12(m) => {
            if let DecodeDifferent::Decoded(modules) = &m.modules {
                for module in modules {
                    set.insert(extract_str(&module.name).to_string());
                }
            }
        }
        V13(m) => {
            if let DecodeDifferent::Decoded(modules) = &m.modules {
                for module in modules {
                    set.insert(extract_str(&module.name).to_string());
                }
            }
        }
        V14(m) => {
            for pallet in &m.pallets {
                set.insert(pallet.name.to_string());
            }
        }
        V15(m) => {
            for pallet in &m.pallets {
                set.insert(pallet.name.to_string());
            }
        }
        _ => {}
    }

    set
}

pub fn check_pallets(meta: &RuntimeMetadataPrefixed, required: &[&str]) -> Result<(), Vec<String>> {
    let available = pallets_in_metadata(meta);
    let missing: Vec<String> = required
        .iter()
        .filter(|p| !available.contains(**p))
        .map(|p| p.to_string())
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}
