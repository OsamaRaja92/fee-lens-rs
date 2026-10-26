//! Constants, and the static compute-unit costs the runtime reserves for builtins.

/// Lamports charged per required signature.
pub const LAMPORTS_PER_SIGNATURE: u64 = 5_000;

/// Micro-lamports in one lamport: the denominator of a compute unit price.
pub const MICRO_LAMPORTS_PER_LAMPORT: u128 = 1_000_000;

/// Units reserved for an instruction whose program is not a builtin.
pub const DEFAULT_INSTRUCTION_COMPUTE_UNITS: u32 = 200_000;

/// The ceiling a single transaction may be budgeted.
pub const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// The program whose instructions set a compute limit and price on legacy and v0.
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";
/// The program whose `Transfer` instruction carries a tip.
pub const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";

/// The allocation SIMD-0170 gives to programs on its builtin list.
///
/// A builtin that is migrated to sBPF is removed from that list and falls back
/// to the 200,000 default, so "migrated" means more compute reserved, not less.
/// The table below is exactly the list the proposal names; anything absent from
/// it — the zk programs, and secp256r1, which postdates the proposal — takes the
/// 200,000 default here.
const BUILTIN_LIST_COMPUTE_UNITS: u32 = 3_000;

/// Builtin programs and the units reserved for one of their instructions.
///
/// These matter only when a legacy or v0 transaction sets a compute unit *price*
/// but no compute unit *limit*. The runtime then derives a limit by walking the
/// instruction list, and because the priority fee is `price x limit`, that
/// derived limit decides the fee.
///
/// The second field marks the entries this crate verifies directly: the fixtures
/// contain transactions whose charged fee is only reproducible with that value.
/// The rest follow the same documented rule but are not exercised by a fixture,
/// and any error there surfaces as a non-zero model residual rather than as a
/// silently wrong number.
pub const BUILTIN_COMPUTE_UNIT_COSTS: &[(&str, u32, bool)] = &[
    (SYSTEM_PROGRAM, BUILTIN_LIST_COMPUTE_UNITS, true),
    (COMPUTE_BUDGET_PROGRAM, BUILTIN_LIST_COMPUTE_UNITS, true),
    (
        "AddressLookupTab1e1111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "Config1111111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "Stake11111111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "Vote111111111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "BPFLoader1111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "BPFLoader2111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "BPFLoaderUpgradeab1e11111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "LoaderV411111111111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "Ed25519SigVerify111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
    (
        "KeccakSecp256k11111111111111111111111111111",
        BUILTIN_LIST_COMPUTE_UNITS,
        false,
    ),
];

/// Units the runtime reserves for one instruction when no limit is declared.
#[must_use]
pub fn reserved_compute_units(program_address: &str) -> u32 {
    BUILTIN_COMPUTE_UNIT_COSTS
        .iter()
        .find(|(address, _, _)| *address == program_address)
        .map_or(DEFAULT_INSTRUCTION_COMPUTE_UNITS, |(_, units, _)| *units)
}
