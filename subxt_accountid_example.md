# Problem: Cannot Distinguish AccountId32 from H256 When Decoding Extrinsics

## Context

When building a Substrate block explorer API compatible with Sidecar, we need to convert `AccountId32` values to SS58 addresses in JSON responses, but NOT convert other 32-byte values like `H256` hashes.

## Current Approach (Doesn't Work)

```rust
use subxt_historic::OnlineClient;
use scale_value::Value;

// Decode extrinsic call fields
let client_at_block = client.at(block_number).await?;
let extrinsics = client_at_block.extrinsics().iter();

for extrinsic in extrinsics {
    let fields = extrinsic.call().fields();

    for field in fields.iter() {
        // Decode to Value<()> - loses type information
        let value: Value<()> = field.decode()?;

        // Now we have a problem:
        // - Both AccountId32 and H256 decode as 32-byte arrays
        // - We can't tell which is which
        // - If we convert all 32-byte values to SS58, we get false positives
    }
}
```

## The Problem

Given this extrinsic call:
```rust
// Staking::nominate call
{
    targets: Vec<MultiAddress<AccountId32, ()>>,  // Should convert to SS58
}

// versus

// System::remark_with_event call
{
    remark: Vec<u8>,  // Contains a 32-byte hash - should NOT convert
}
```

Both decode to the same structure when using `Value<()>`:
```json
{
    "targets": [
        { "id": "0x00964d74f8027e07b43717b6876d97544fe0d71facef06acc83827499e944e" }
    ]
}

{
    "remark": "0x742f54c6e469e30e27dbfbaff35166ae00d7e9a40906485ba57c06483292cf39"
}
```

We can't tell that the first is an `AccountId32` (should convert to SS58) and the second is just bytes (should stay as hex).

## What We Need

We need to preserve type IDs during decoding so we can check the metadata to determine if a value is an `AccountId32`:

```rust
// Ideally something like:
let value: Value<u32> = field.decode_with_type_id()?;
// where value.context contains the type ID from the metadata

// Then we can check:
if is_account_id_type(registry, value.context) {
    convert_to_ss58(&value);
}
```

## Attempted Solution

We tried using `scale_value::scale::decode_as_type()` directly:

```rust
// In subxt-historic, added method to ExtrinsicCallField:
pub fn decode_with_type_id(&self) -> Result<Value<u32>, Error> {
    match &self.info {
        AnyExtrinsicCallFieldInfo::Current(info) => {
            let cursor = &mut &*self.field_bytes;
            let type_id = *info.info.ty();

            // This works and preserves type IDs
            let decoded = scale_value::scale::decode_as_type(
                cursor,
                type_id,
                info.resolver
            )?;

            Ok(decoded)
        }
        // ... legacy handling
    }
}
```

## The Problem With Our Solution

When we use `decode_as_type()`, byte arrays get decoded as individual `Primitive::U128` values:

```rust
// AccountId32 becomes:
Value {
    context: 123,  // type ID for AccountId32
    value: Composite::Unnamed([
        Value { context: 456, value: Primitive::U128(0) },    // byte 0
        Value { context: 456, value: Primitive::U128(150) },  // byte 1
        // ... 30 more individual bytes
    ])
}
```

This makes serialization extremely difficult and breaks other byte array fields.

## Question

**Is there a better way to decode extrinsic fields while preserving type information, without having byte arrays decompose into individual primitive values?**

Or alternatively:
- Should we decode differently?
- Is there metadata we can access before decoding to determine the field type?
- Should we use a different decoding strategy for this use case?

## Minimal Reproduction

```rust
use subxt_historic::OnlineClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = OnlineClient::from_url(
        SubstrateConfig::default(),
        "wss://rpc.polkadot.io"
    ).await?;

    // Block with Staking::nominate call
    let client_at_block = client.at(24500000).await?;
    let extrinsics = client_at_block.extrinsics().iter();

    for extrinsic in extrinsics {
        if extrinsic.call().pallet_name() == "Staking"
            && extrinsic.call().name() == "nominate"
        {
            let fields = extrinsic.call().fields();

            for field in fields.iter() {
                println!("Field: {}", field.name());

                // Current approach - no type info
                let value_no_type: Value<()> = field.decode()?;
                println!("Without type: {:?}", value_no_type);

                // Desired approach - with type info
                // But this decomposes byte arrays
                let value_with_type: Value<u32> = field.decode_with_type_id()?;
                println!("With type ID: {:?}", value_with_type);
            }
        }
    }

    Ok(())
}
```

**Expected behavior**: We can identify that the `targets` field contains `AccountId32` values and convert them to SS58 addresses, while leaving other 32-byte values (like block hashes) as hex strings.

**Actual behavior**: Either we lose type information (can't distinguish types) or byte arrays decompose into individual U128 primitives (unusable for serialization).
