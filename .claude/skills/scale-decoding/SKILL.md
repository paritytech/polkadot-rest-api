---
name: scale-decoding
description: Use when decoding SCALE bytes, defining value types for dynamic storage queries, or choosing between parity_scale_codec::Decode and scale_decode::DecodeAsType — including when decoded values look subtly wrong without any error
---

# SCALE decoding practices

## Storage query typing

```rust
let addr = subxt::dynamic::storage::<(), u32>("Referenda", "ReferendumCount");
let value = at_block.storage().fetch(addr, ()).await?; // second fetch arg = key tuple; () for plain entries
let count: u32 = value.decode()?;
```

The second type parameter on the address is the value type `.decode()` will produce:

- Not going to call `.decode()`? Use `()` to say so.
- Have your own type? Derive `scale_decode::DecodeAsType` on it and use it there, or call `.decode_as::<MyType>()` on the fetched value to decode into an arbitrary `DecodeAsType` type.
- Prefer concrete types over `scale_value::Value` where the shape is known — `Value` is for genuinely dynamic data.

## Decode vs DecodeAsType

| | `parity_scale_codec::Decode` | `scale_decode::DecodeAsType` |
|---|---|---|
| Speed | very fast | slightly slower |
| Type info | none needed | required (from metadata) |
| Layout mismatch | silently produces wrong data | a genuine mismatch errors; only *compatible* differences are absorbed (skips fields you don't read, widens e.g. u8→u32, unwraps newtypes) |
| Failure mode | can "succeed" with garbage | errors — or exposes missing type info (report upstream so it gets fixed) |

**Default to `DecodeAsType` for chain data** — its layout varies across runtimes. Raw `Decode` is fine for types whose encoding you fully control, and also for deliberate hot-path decodes of stable chain types (e.g. `Era`, fee `RuntimeDispatchInfo`) where the metadata-driven path is too slow — several exist in this repo by design, so don't migrate them without a reason.

## Leftover bytes = something went wrong

With `Decode`, leftover bytes after decoding (almost always) mean your target type doesn't match the encoded bytes — and plain `decode()` won't tell you. Use `DecodeAll` instead, which errors on trailing bytes:

```rust
use parity_scale_codec::DecodeAll;
let val = MyType::decode_all(&mut &bytes[..])?;
```

(In rare cases leftover bytes mean the on-chain storage itself is corrupt — still worth surfacing, never worth ignoring.)

The two fixes above are complementary, not alternatives: prefer `DecodeAsType` wherever metadata type info exists; on paths where raw `Decode` must stay, swap plain `decode()` for `decode_all()` **only when you decode a whole buffer**. Do not use `decode_all` for cursor parsing — code that decodes one field from `&mut &bytes[..]` and advances through a larger buffer (e.g. the era/signature parsing in `utils/extrinsic.rs`) leaves trailing bytes on purpose, and `decode_all` would reject them.
