# xmip-core-contract-edi-edifact

The UN/EDIFACT content contract, a technology of
[xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Two claims. **Well-formedness is a given**: a sound interchange by ISO 9735,
service characters and release honored, `UNB`/`UNZ`, `UNH`/`UNT` and
`UNG`/`UNE` paired with matching references and counts. **Conformance is a given
once the contract is named**: a Receive or Send Location that refers to this
contract with a message type bound, `ORDERS` or `ORDERS:D:96A`, has every
message's `UNH` held to it.

An interchange is read by the Foundation's one segment walk
([xmip-core-message](https://github.com/IlleNilsson/xmip-core-message)'s
`segment`, which the EDIFACT shape sections by too), a segment at a time in
the service characters the `UNA` advises or the default set, releases
honored: `syntax::Interchange::walk` hands it the service characters and
holds each tag to three capitals or digits. The `UNA` is read by the EDIFACT
shape's one reader
([xmip-core-message-edi-edifact](https://github.com/IlleNilsson/xmip-core-message-edi-edifact)'s
`advice`), the shape of the same standard and the one technology this
contract depends on (ADR-0044, amendment 2026-11-15): a space advised as the
release character is none, as ISO 9735 syntax 3 says, so a value holding a
service character is refused on write. The contract walks the
interchange whole into the segments it checks, and the `edi` path technology
([xmip-core-path-edi](https://github.com/IlleNilsson/xmip-core-path-edi))
cuts only the segment it reads and walks the rest uncut, so a malformed
interchange is refused in the same words by both.

The claims are this crate's when it is called, and the System Tests call it.
A node holding a Location's Streams to it is
[decided, not built](../../../../../doc/architecture/estate-map.md#arrival-validation): a node refuses to start a
Location that names a contract until it does.

Every EDIFACT directory is a version of this one technology and lives here; the
segment tables that hold a message to its directory's structure are the next
layer in this repository.

## The declaration

What a Location naming this contract is configured with is two `const`s in
its `declaration` module, `src/declaration.rs`, beside the code, which its
`ContractFactory` reads (ADR-0064, amendment 2026-11-26):
`declaration::SETTINGS`, `reference`, the message type an interchange
carries, and `NAME`, `edi-edifact`, the name a Validate step's `format`
names it by. Built with its `declaration` feature alone
(`--no-default-features --features declaration`), the crate is those and
nothing more: its code and every crate the code is built on are behind the
default feature. So the library the surfaces load can carry it without the
code, for the language server and the desktop to suggest and check a
Location's contract settings though no node in their process loaded the
technology, once the gathering crate of open problem 48 links it.
`test/Declaration.Test.ps1` in the estate holds that the declaration builds
nothing the contract capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
