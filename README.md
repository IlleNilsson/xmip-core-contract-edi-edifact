# xmip-core-contract-edi-edifact

The UN/EDIFACT content contract, a technology of
[xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Two claims. **Well-formedness is a given**: a sound interchange by ISO 9735,
service characters and release honored, `UNB`/`UNZ`, `UNH`/`UNT` and
`UNG`/`UNE` paired with matching references and counts. **Conformance is a given
once the contract is named**: a Receive or Send Location that refers to this
contract with a message type bound, `ORDERS` or `ORDERS:D:96A`, has every
message's `UNH` held to it.

Every EDIFACT directory is a version of this one technology and lives here; the
segment tables that hold a message to its directory's structure are the next
layer in this repository.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
